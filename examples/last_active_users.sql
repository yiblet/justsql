-- @import all_users from './all_users.sql'
WITH all_u as (
	@all_users()
)
select * from all_u
