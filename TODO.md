irc features:
- [ ] nickname of the user who changed the topic
- [ ] support WHO, LIST, NAMES, ...
- [ ] support server password (PASS)
- [ ] support NICK

main:
- [ ] move `ServerState` to its own module
- [ ] move `Sessions` to their own modules
- [ ] cleanup `ConnectingSession::connect_user` and welcome message

state:
- [ ] keep track of connecting users (`ServerState::connecting_users`)
- [ ] transactions for nicknames at connection
- [ ] unit tests

session:
- [ ] handle disconnection w.r.t. the state and mailbox
- [ ] make sure connection is still alive (send pings)
- [ ] drop spam connections (maybe only in `ConnectingSession`)

client_to_server:
- [ ] validate ChannelID strings (must start with `#` or...) (also to distinguish between channel target and user target for PRIVMSG)
- [ ] nicer error when converting to `client_to_server::Message`
- [ ] remove the parameters indexing (`parameters()[0]`) as it can panic
- [ ] unit tests

server_to_client:

parser:
- [ ] zero-copy
