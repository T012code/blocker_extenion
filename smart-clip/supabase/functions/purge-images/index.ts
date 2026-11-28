// Smart Clip: removes image files that no clip points to any more (expired after
// 24 hours, pushed out by the per-account cap, or deleted). Called hourly by pg_cron
// (see ../../schedule-images.sql). Storage files can only be deleted through the
// Storage API, which is why this isn't plain SQL.
//
// The caller must send the shared secret in the x-purge-secret header. It's set as the
// PURGE_SECRET function secret and kept in Vault for the scheduled job.
import { createClient } from "npm:@supabase/supabase-js@2";

const url = Deno.env.get("SUPABASE_URL")!;
const serviceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!;
const secret = Deno.env.get("PURGE_SECRET") ?? "";

/** Compares two strings in constant time. */
function same(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

Deno.serve(async (req) => {
  if (req.method !== "POST") return new Response("Use POST", { status: 405 });
  if (secret.length < 32 || !same(req.headers.get("x-purge-secret") ?? "", secret)) {
    return new Response("Not allowed", { status: 401 });
  }
  const db = createClient(url, serviceKey, { auth: { persistSession: false } });
  let removed = 0;
  // A few rounds of up to 500 files keep each Storage call small.
  for (let round = 0; round < 10; round++) {
    const { data, error } = await db.rpc("smart_clip_orphan_images", { max_rows: 500 });
    if (error) return Response.json({ removed, error: error.message }, { status: 500 });
    const names = (data ?? []) as string[];
    if (names.length === 0) break;
    const { error: rmError } = await db.storage.from("clips").remove(names);
    if (rmError) return Response.json({ removed, error: rmError.message }, { status: 500 });
    removed += names.length;
    if (names.length < 500) break;
  }
  return Response.json({ removed });
});
