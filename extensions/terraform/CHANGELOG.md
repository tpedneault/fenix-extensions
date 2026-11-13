# Changelog

## 1.0.0

- The first release, for Fenix 1.5 (extension API 1.5): Terraform and HCL.
  Their grammar (`.tf`, `.tfvars` and `.hcl` files: highlighted, folded,
  indented, with their blocks as the scopes `SPC c s` and the breadcrumbs
  list), terraform-ls as the language server, `terraform fmt` as the
  formatter, `terraform validate` on save, and tasks: `init`, `validate`,
  `fmt -check`, `plan` and `apply saved plan`, which applies the plan file
  `plan` wrote and nothing else. The project doctor checks Terraform and
  terraform-ls, with a fix that installs terraform-ls 0.39.1 from
  HashiCorp's release, checked against its SHA-256. Snippets, a *Terraform ·
  root module* project for the new-project wizard, and a lesson in the
  tutorial.
