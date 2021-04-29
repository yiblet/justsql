-- the auth keyword has the following 3 possible types:
-- 		authorize -- where you set up an auth token
-- 		verify    -- where you can verify (and optionally reissue) tokens
--    clear     -- where you can clear the http only token
-- here's an example that let's the user login 
--
-- @auth authorize 2d
-- @endpoint login
-- @param email
select @email
