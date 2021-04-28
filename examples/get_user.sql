-- gets a user's info by it's email
-- @endpoint getUser
-- @param email
select * from users 
where email = @email 
