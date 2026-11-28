-- Smart Clip database tests. Run with run-tests.sh. Any failed check raises an error.
\set ON_ERROR_STOP on
\set QUIET on

create function pg_temp.check(ok boolean, what text) returns void language plpgsql as $$
begin
  if not coalesce(ok, false) then raise exception 'FAILED: %', what; end if;
  raise notice 'ok   %', what;
end $$;

-- Acting as a signed-in person (as PostgREST does) or as nobody.
create function pg_temp.as_user(id uuid) returns void language plpgsql as $$
begin
  perform set_config('request.jwt.claim.sub', id::text, false);
  execute 'set role authenticated';
end $$;

insert into auth.users values
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'),
  ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb');
\set A '''aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'''
\set B '''bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'''
\set DEV '''11111111-1111-4111-8111-111111111111'''

-- The upsert PostgREST runs for ?on_conflict=user_id,hash with merge-duplicates.
create function pg_temp.upsert(k text, h text, t text, img text default null, th text default null)
returns uuid language sql as $$
  insert into public.clips (device_id, device_name, kind, hash, text, image_path, thumb)
  values ('11111111-1111-4111-8111-111111111111', 'Desk PC', k, h, t, img, th)
  on conflict (user_id, hash) do update set
    device_id = excluded.device_id, device_name = excluded.device_name, kind = excluded.kind,
    text = excluded.text, image_path = excluded.image_path, thumb = excluded.thumb
  returning id
$$;
grant execute on function pg_temp.upsert(text, text, text, text, text) to authenticated;
grant execute on function pg_temp.check(boolean, text) to authenticated;

-- ------------------------------------------------------------ a text clip
select pg_temp.as_user(:A);
select pg_temp.upsert('text', repeat('a', 64), 'hello') as first_id \gset
select pg_temp.check(
  (select expires_at - created_at = interval '7 days' and user_id = :A from public.clips),
  'a text clip belongs to its account and lasts 7 days');
reset role;
select pg_temp.check(
  (select count(*) = 1 and bool_and(topic = 'clips:' || :A and event = 'INSERT' and payload->>'text' = 'hello' and private)
     from realtime.messages),
  'a new clip is pushed to the account''s private channel');

-- ------------------------------------------------------------ copying the same thing again
update public.clips set created_at = created_at; -- (the stamp trigger keeps times server-set)
select pg_sleep(0.01);
select pg_temp.as_user(:A);
select pg_temp.upsert('text', repeat('a', 64), 'hello') as again_id \gset
select pg_temp.check(:'first_id' = :'again_id' and (select count(*) = 1 from public.clips),
  'copying the same text again bumps the clip instead of adding one');
reset role;
select pg_temp.check((select event = 'UPDATE' from realtime.messages order by id desc limit 1),
  'the bump is pushed too');

-- ------------------------------------------------------------ privacy between accounts
select pg_temp.as_user(:B);
select pg_temp.check((select count(*) = 0 from public.clips), 'another account sees none of them');
update public.clips set text = 'hacked';
delete from public.clips;
reset role;
select pg_temp.check((select text = 'hello' from public.clips), 'another account can''t change or delete them');
select pg_temp.as_user(:B);
do $$ begin
  insert into public.clips (user_id, device_id, device_name, kind, hash, text)
  values ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', gen_random_uuid(), 'x', 'text', repeat('b', 64), 'spoof');
  raise exception 'FAILED: wrote a clip into another account';
exception when insufficient_privilege then raise notice 'ok   can''t write clips into another account';
end $$;
set realtime.topic = 'clips:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
select pg_temp.check((select count(*) = 0 from realtime.messages), 'can''t listen to another account''s channel');
reset role;
select pg_temp.as_user(:A);
select pg_temp.check((select count(*) > 0 from realtime.messages), 'can listen to its own channel');
reset realtime.topic;
reset role;
set role anon;
select pg_temp.check(not has_table_privilege('anon', 'public.clips', 'select'), 'signed-out visitors have no access');
reset role;

-- ------------------------------------------------------------ images
select pg_temp.as_user(:A);
select pg_temp.upsert('image', repeat('c', 64), null, :A::text || '/x.webp', 'AAAA');
select pg_temp.check(
  (select expires_at - created_at = interval '24 hours' from public.clips where kind = 'image'),
  'an image clip lasts 24 hours');
do $$ begin
  perform pg_temp.upsert('image', repeat('d', 64), null, 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/x.webp', 'AAAA');
  raise exception 'FAILED: pointed at another account''s image folder';
exception when insufficient_privilege then raise notice 'ok   an image clip can only point into its own folder';
end $$;
do $$ begin
  perform pg_temp.upsert('text', repeat('e', 64), 'x', 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa/y.webp', null);
  raise exception 'FAILED: a text clip with an image';
exception when check_violation then raise notice 'ok   clips must be either text or an image';
end $$;
do $$ begin
  perform pg_temp.upsert('text', 'not-a-hash', 'x');
  raise exception 'FAILED: a bad hash';
exception when check_violation then raise notice 'ok   hashes must be SHA-256 hex';
end $$;
do $$ begin
  perform pg_temp.upsert('text', repeat('f', 64), repeat('x', 102401));
  raise exception 'FAILED: text over 100 KB';
exception when check_violation then raise notice 'ok   text over 100 KB is refused';
end $$;
insert into storage.objects (bucket_id, name) values ('clips', :A::text || '/x.webp');
do $$ begin
  insert into storage.objects (bucket_id, name) values ('clips', 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/z.webp');
  raise exception 'FAILED: uploaded into another account''s folder';
exception when insufficient_privilege then raise notice 'ok   image files can only go in the account''s own folder';
end $$;
reset role;
select pg_temp.check((select public = false and file_size_limit = 2097152 from storage.buckets where id = 'clips'),
  'the image bucket is private and capped at 2 MB');

-- ------------------------------------------------------------ the caps (200 clips, 30 images)
truncate realtime.messages;
select pg_temp.as_user(:A);
select count(pg_temp.upsert('text', lpad(to_hex(i), 64, '0'), 't' || i)) from generate_series(1, 205) i;
select count(pg_temp.upsert('image', lpad(to_hex(1000 + i), 64, '1'), null, :A::text || '/i' || i || '.webp', 'AAAA'))
from generate_series(1, 35) i;
select pg_temp.check((select count(*) <= 200 from public.clips), 'an account keeps at most 200 clips');
select pg_temp.check((select count(*) = 30 from public.clips where kind = 'image'), '...of which at most 30 images');
select pg_temp.check(
  not exists (select 1 from public.clips where text = 't1')
  and exists (select 1 from public.clips where text = 't205')
  and exists (select 1 from public.clips where image_path like '%/i35.webp')
  and not exists (select 1 from public.clips where image_path like '%/i1.webp'),
  'the oldest go first, the newest always stay');
reset role;
select pg_temp.check((select count(*) = 0 from realtime.messages where event = 'DELETE'),
  'cap deletions aren''t pushed (each PC trims its own list)');

-- ------------------------------------------------------------ deleting
select pg_temp.as_user(:A);
select id as one from public.clips where kind = 'text' limit 1 \gset
delete from public.clips where id = :'one';
reset role;
select pg_temp.check(
  (select payload = jsonb_build_object('id', :'one'::uuid) from realtime.messages where event = 'DELETE'),
  'a deletion is pushed as just the id');

-- ------------------------------------------------------------ hourly clean-up
alter table public.clips disable trigger clips_stamp;
update public.clips set expires_at = now() - interval '1 minute' where kind = 'image';
alter table public.clips enable trigger clips_stamp;
truncate realtime.messages;
select pg_temp.check((select public.smart_clip_purge() = 30), 'the clean-up deletes expired clips');
select pg_temp.check((select count(*) = 0 from realtime.messages), '...without pushing every deletion');
update storage.objects set created_at = now() - interval '1 hour';
set role service_role;
select pg_temp.check(
  (select array_agg(n) = array[:A::text || '/x.webp'] from public.smart_clip_orphan_images(500) n),
  'image files no clip points to are found for removal');
reset role;
select pg_temp.check(
  not has_function_privilege('authenticated', 'public.smart_clip_purge()', 'execute')
  and not has_function_privilege('anon', 'public.smart_clip_orphan_images(integer)', 'execute')
  and not has_function_privilege('authenticated', 'public.smart_clip_orphan_images(integer)', 'execute'),
  'only the server can run the clean-up');
select pg_temp.check((select count(*) = 1 from cron.job where jobname = 'smart-clip-purge-clips' and schedule = '7 * * * *'),
  'the clean-up runs every hour');

-- ------------------------------------------------------------ deleting an account
delete from auth.users where id = :A;
select pg_temp.check((select count(*) = 0 from public.clips), 'deleting an account deletes its clips');

\echo
\echo all database tests passed
