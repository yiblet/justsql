# "It's Just SQL"

justsql is a simple easily configurable server that aims to remove boring backend CRUD work for building web apps.
Writing modern CRUD code is tedious. Every time you need to create a new model for your app, you need to write an ever expanding number of lines before you can get to using it in the frontend.

Take for example something as simple as a user's personalization settings. Starting from scratch, it can take painfully long before you have it ready to use for the frontend.

1. Create a new table pointing with a foreign key to the user's table.
2. Write the "create, read, update and delete" functions that use your ORM to modify the table.
3. Write the endpoints to read and modify this information such as:

- `GET /api/v1/user/:userId/settings` for reading
- `POST /api/v1/user/:userId/settings` for creating
- `UPDATE /api/v1/user/:userId/settings` and updating

4. Add validation code to these endpoints.
5. Add these functions to query these endpoints to the client code.

When it can take days to create apis for your frontend, quick iterative development grinds to a halt. `justsql` aims to be the solution to this problem. The idea is that you can now describe everything you need
to make a CRUD endpoint in a less than 10 line file of sql.

To request the user's settings in justsql it takes just these 6 lines:

```sql
-- @endpoint get_user_settings
-- @auth verify
-- @param user_id
SELECT user_id, dark_mode_enabled, default_text_size, language_preference FROM user_settings
WHERE user_id = @user_id::INT4
LIMIT 1
```

Updates and creations are similarly embarrasingly simple. With this level of speed, the time it takes to get a new table up and into use on the
frontend turns from potentially entire evenings to just a couple minutes. Coupling justsql with a database tools like dbeaver, sqitch, you can move much
faster in developing your product, and take the CRUD out of your backend.

This isn't meant to replace your backend. Clearly, you can't send an email notification with justsql. Instead, it's supposed to reduce the backend down to just
the interesting parts. Ideally, you'll now can add whole endpoints and change what data you access, without a single commit to your backend codebase.

## Status of the project.

justsql is still at this point alpha-quality code. There's still a lot left to do before it becomes the amazing tool it can be, but right now you can already try 
it out on a couple of toy situations to get a feel for what it'd be like. 

Star this repo and get updates as the project gets going if you want to try it out once it's ready for production. 
