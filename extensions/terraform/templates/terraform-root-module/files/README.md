# {{name}}

A Terraform root module, started from Fenix's *Terraform · root module* template.

| File | What it holds |
|---|---|
| `main.tf` | the resources and data sources |
| `versions.tf` | the Terraform version and the providers it needs |
| `variables.tf` | its inputs |
| `outputs.tf` | what it tells you |

In Fenix, `SPC p t` offers the tasks, in this order: `init`, `validate`,
`fmt -check`, `plan` and `apply saved plan`. `plan` writes what it would do
to `tfplan`; `apply saved plan` does exactly that and nothing else, to
real infrastructure, so read the plan first. Saving a `.tf` file runs
`terraform validate` on it, and `SPC c F` runs `terraform fmt`.

Commit `.terraform.lock.hcl`; keep state out of Git (use a backend) and
never commit `tfplan`.
