-- Smart Clip: run the purge-images Edge Function every hour.
-- Run this once, after deploying purge-images and setting its PURGE_SECRET.
-- Replace both <...> values first. <project-ref> is in the project URL
-- (https://<project-ref>.supabase.co); <purge-secret> is the same long random
-- string you set as the function's PURGE_SECRET (at least 32 characters).

create extension if not exists pg_net;

select vault.create_secret('<purge-secret>', 'smart_clip_purge_secret');
select vault.create_secret('https://<project-ref>.supabase.co/functions/v1/purge-images', 'smart_clip_purge_url');

select cron.schedule(
  'smart-clip-purge-images',
  '17 * * * *',
  $$
  select net.http_post(
    url := (select decrypted_secret from vault.decrypted_secrets where name = 'smart_clip_purge_url'),
    headers := jsonb_build_object(
      'Content-Type', 'application/json',
      'x-purge-secret', (select decrypted_secret from vault.decrypted_secrets where name = 'smart_clip_purge_secret')
    ),
    body := '{}'::jsonb,
    timeout_milliseconds := 30000
  );
  $$
);
