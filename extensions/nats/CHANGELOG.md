# Changelog

## 1.1.0

- `nats.open`, `nats.jetstream` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.2: a NATS client -- live subjects with their rates and messages, payloads pretty-printed, a subject tree, filters that become the server's subscriptions, highlight rules that notify, publish and request from a JSON buffer, messages sent again or exported as JSONL, JetStream's streams, consumers and key-value buckets, credentials in the keychain (user, token, NKey, .creds), and tools for coding agents -- storing within a memory budget and sampling under load, so a busy server never finds Fenix slow.
