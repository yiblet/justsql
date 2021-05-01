-- @param email
-- @param password
INSERT INTO users (email, password) VALUES (
  @email::TEXT,
  crypt(@password::TEXT, gen_salt('bf')) -- @password  fails here because there's no space 
)
RETURNING id, email;
