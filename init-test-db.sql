-- Create the test database alongside the prod DB on first init
CREATE DATABASE diminish_test;
GRANT ALL PRIVILEGES ON DATABASE diminish_test TO dev;
