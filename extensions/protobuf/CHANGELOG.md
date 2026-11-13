# Changelog

## 1.2.0

- For Fenix 1.5 (extension API 1.5): how installing buf, protoc or
  grpcurl ended is a notification rather than a line in the message
  line, since the download can take a while and you may be elsewhere --
  "Installed buf 1.75.0", and the folder it's in; or "Couldn't install
  buf 1.75.0", saying whether its download, unpacking it or setting it
  up failed and why, with *Try again*. The newest for a tool takes the
  last one's place, and a failure's is taken away when you try again.
  Starting an install is still said in the message line.
- The two messages that name a key name it as the key scheme you use has
  it: "choose a method first" says the keys that open the client (`SPC m
  g`, `C-k m g` with standard keys, or the ones you gave it), and the
  client's "grpcurl isn't installed" says the project doctor's. That one
  said `SPC p d`, which is Fenix's *delete project*; the doctor is `SPC
  p h`. The README gives standard keys beside Vim's. On Fenix 1.0 to
  1.4, 1.1.0 stays the one you have.

## 1.1.0

- `protobuf.client` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: Protocol Buffers and gRPC: the grammar, buf's language server, buf format, buf lint on save, buf breaking against the default branch, buf generate and buf build as tasks, the project's checks with fixes that install buf, protoc and grpcurl, snippets, and a gRPC client -- a server's services by reflection or the project's protos, a body made from the method's message, server streams as they come, status, trailers, timing and history.
