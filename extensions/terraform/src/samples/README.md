# Samples

What the tests read:

- `orbit.tf`: Terraform the grammar's queries run on.
- `terraform-version.txt`, `terraform-ls-version.txt`: `terraform version`
  and `terraform-ls version`, recorded from Terraform 1.17.2 and
  terraform-ls 0.39.1 on linux_amd64.
- `validate-*.json`: `terraform validate -json`, recorded from Terraform
  1.17.2, in small configurations made for it: `errors` (two references to
  what isn't declared, in the top module), `nested` (run with
  `-chdir=modules/net`: file names are relative to that folder), `syntax`
  (a block never closed), `warning` (a deprecated variable given a value:
  valid, exit 0), `child-error` (a warning in the module that calls and an
  error in the module called), `valid`, `module-not-installed` (a module
  block before `init`) and `missing-provider` (a provider `terraform init`
  couldn't download), which has no `range` at all.

The versions' output and the JSON are real Terraform's, not written by
hand.
