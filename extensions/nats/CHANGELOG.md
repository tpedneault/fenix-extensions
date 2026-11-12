# Changelog

## 1.2.0

- For Fenix 1.5 (extension API 1.5): what it says names the keys of the
  key scheme in use, and yours when you've bound your own -- the empty
  page's "then `C-k o m c` connects" with standard keys, `SPC o m c` with
  Vim keys, and likewise the publish buffer's title, the hints to set a
  server's credentials and the errors that send you to Fenix's settings
  (`C-,`, `SPC ,`). The README gives standard keys beside Vim's.

## 1.1.0

- `nats.open`, `nats.jetstream` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.2: a NATS client -- live subjects with their rates and messages, payloads pretty-printed, a subject tree, filters that become the server's subscriptions, highlight rules that notify, publish and request from a JSON buffer, messages sent again or exported as JSONL, JetStream's streams, consumers and key-value buckets, credentials in the keychain (user, token, NKey, .creds), and tools for coding agents -- storing within a memory budget and sampling under load, so a busy server never finds Fenix slow.
