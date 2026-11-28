-- Smart Clip: the clips a person shares between their PCs.
--
-- What this sets up:
--   * public.clips: one row per clip. Text clips last 7 days, image clips 24 hours.
--     Each account keeps at most 200 clips, of which at most 30 are images.
--   * Storage bucket "clips": the images themselves (WebP or JPEG, up to 2 MB), in a
--     folder named after the account. Rows only hold a small thumbnail and the path.
--   * Live updates: a private Realtime Broadcast channel per account ("clips:<user id>").
--     New and changed clips are pushed to the account's other PCs; deletions send just the id.
--   * Clean-up: an hourly job deletes expired rows. Image files are removed by the
--     purge-images Edge Function (see schedule-images.sql), because Storage files can
--     only be deleted through the Storage API.
--   * Row-level security everywhere: an account only ever sees and changes its own clips.

-- ---------------------------------------------------------------- clips table

create table public.clips (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null default auth.uid() references auth.users (id) on delete cascade,
  device_id uuid not null,
  device_name text not null check (char_length(device_name) between 1 and 64),
  kind text not null check (kind in ('text', 'image')),
  -- SHA-256 of the content, in hex: copying the same thing again bumps the old clip.
  hash text not null check (hash ~ '^[0-9a-f]{64}$'),
  text text check (octet_length(text) <= 102400),
  -- Images: a small JPEG thumbnail (base64) and the file's path in the "clips" bucket.
  thumb text check (octet_length(thumb) <= 24000),
  image_path text check (char_length(image_path) <= 200),
  width integer check (width between 1 and 20000),
  height integer check (height between 1 and 20000),
  bytes integer not null default 0 check (bytes between 0 and 2097152),
  created_at timestamptz not null default now(),
  expires_at timestamptz not null default now(),
  constraint clips_shape check (
    (kind = 'text' and text is not null and image_path is null and thumb is null)
    or (kind = 'image' and text is null and image_path is not null and thumb is not null)
  ),
  constraint clips_user_hash unique (user_id, hash)
);

comment on table public.clips is 'Smart Clip: clips shared between one account''s PCs. Text lasts 7 days, images 24 hours.';

-- The app lists an account's newest clips; the clean-up job looks for expired ones.
create index clips_user_created on public.clips (user_id, created_at desc);
create index clips_expires on public.clips (expires_at);
create index clips_image_path on public.clips (image_path) where image_path is not null;

-- Times are always set here, never by the app: a copy (or a repeat copy) starts the clock.
-- clock_timestamp() (not now()) so clips saved in one transaction still have an order.
create function public.smart_clip_stamp()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if tg_op = 'UPDATE' then
    new.id := old.id;
    new.user_id := old.user_id;
  end if;
  new.created_at := clock_timestamp();
  new.expires_at := new.created_at + case new.kind when 'image' then interval '24 hours' else interval '7 days' end;
  return new;
end;
$$;

create trigger clips_stamp
before insert or update on public.clips
for each row execute function public.smart_clip_stamp();

-- ---------------------------------------------------------------- live updates

-- Pushes each change to the account's private channel. Deletions made by the clean-up
-- job or by the per-account cap are not sent: every PC drops those clips on its own,
-- so sending them would only use up Realtime messages.
create function public.smart_clip_broadcast()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  if coalesce(current_setting('smart_clip.quiet', true), 'off') = 'on' then
    return null;
  end if;
  if tg_op = 'DELETE' then
    perform realtime.send(jsonb_build_object('id', old.id), 'DELETE', 'clips:' || old.user_id::text, true);
  else
    perform realtime.send(to_jsonb(new), tg_op, 'clips:' || new.user_id::text, true);
  end if;
  return null;
end;
$$;

-- AFTER triggers run in name order: the broadcast (a_) goes before the cap (b_).
create trigger clips_a_broadcast
after insert or update or delete on public.clips
for each row execute function public.smart_clip_broadcast();

-- At most 200 clips per account, and at most 30 of them images. The oldest go first.
create function public.smart_clip_cap()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  perform set_config('smart_clip.quiet', 'on', true);
  delete from public.clips
  where id in (
    select id from public.clips
    where user_id = new.user_id
    order by created_at desc
    offset 200
  );
  if new.kind = 'image' then
    delete from public.clips
    where id in (
      select id from public.clips
      where user_id = new.user_id and kind = 'image'
      order by created_at desc
      offset 30
    );
  end if;
  perform set_config('smart_clip.quiet', 'off', true);
  return null;
end;
$$;

create trigger clips_b_cap
after insert on public.clips
for each row execute function public.smart_clip_cap();

-- ---------------------------------------------------------------- security

alter table public.clips enable row level security;

revoke all on public.clips from anon;

create policy "Own clips: read"
on public.clips for select to authenticated
using ((select auth.uid()) = user_id);

create policy "Own clips: add"
on public.clips for insert to authenticated
with check (
  (select auth.uid()) = user_id
  and (image_path is null or image_path like (select auth.uid())::text || '/%')
);

create policy "Own clips: bump"
on public.clips for update to authenticated
using ((select auth.uid()) = user_id)
with check (
  (select auth.uid()) = user_id
  and (image_path is null or image_path like (select auth.uid())::text || '/%')
);

create policy "Own clips: remove"
on public.clips for delete to authenticated
using ((select auth.uid()) = user_id);

-- Each account may listen only to its own channel.
create policy "Own clips channel: listen"
on realtime.messages for select to authenticated
using (
  realtime.topic() = 'clips:' || (select auth.uid())::text
  and realtime.messages.extension = 'broadcast'
);

-- ---------------------------------------------------------------- images

insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values ('clips', 'clips', false, 2097152, array['image/webp', 'image/jpeg']);

create policy "Own clip images: read"
on storage.objects for select to authenticated
using (bucket_id = 'clips' and (storage.foldername(name))[1] = (select auth.uid())::text);

create policy "Own clip images: add"
on storage.objects for insert to authenticated
with check (bucket_id = 'clips' and (storage.foldername(name))[1] = (select auth.uid())::text);

create policy "Own clip images: remove"
on storage.objects for delete to authenticated
using (bucket_id = 'clips' and (storage.foldername(name))[1] = (select auth.uid())::text);

-- ---------------------------------------------------------------- clean-up

-- Deletes expired clips. Run hourly by pg_cron (below).
create function public.smart_clip_purge()
returns integer
language plpgsql
security definer
set search_path = ''
as $$
declare
  removed integer;
begin
  perform set_config('smart_clip.quiet', 'on', true);
  delete from public.clips where expires_at < now();
  get diagnostics removed = row_count;
  perform set_config('smart_clip.quiet', 'off', true);
  return removed;
end;
$$;

-- Image files no clip points to any more (expired, capped or deleted). The 15-minute
-- grace keeps a file that was just uploaded and whose clip is still being saved.
create function public.smart_clip_orphan_images(max_rows integer default 500)
returns setof text
language sql
stable
security definer
set search_path = ''
as $$
  select o.name
  from storage.objects o
  where o.bucket_id = 'clips'
    and o.created_at < now() - interval '15 minutes'
    and not exists (select 1 from public.clips c where c.image_path = o.name)
  order by o.created_at
  limit greatest(1, least(max_rows, 1000));
$$;

-- Only the server may run these.
revoke execute on function public.smart_clip_purge() from public, anon, authenticated;
revoke execute on function public.smart_clip_orphan_images(integer) from public, anon, authenticated;
grant execute on function public.smart_clip_orphan_images(integer) to service_role;
revoke execute on function public.smart_clip_stamp() from public, anon, authenticated;
revoke execute on function public.smart_clip_broadcast() from public, anon, authenticated;
revoke execute on function public.smart_clip_cap() from public, anon, authenticated;

create extension if not exists pg_cron;

select cron.schedule('smart-clip-purge-clips', '7 * * * *', 'select public.smart_clip_purge()');
