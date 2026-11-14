# Terraform

Terraform and HCL in Fenix: their grammar (Fenix has none of its own),
terraform-ls, `terraform fmt`, `terraform validate` on save, `init`,
`validate`, `plan` and `apply` as tasks, the project doctor's checks with a
fix that installs terraform-ls, snippets, and a root-module project in the
new-project wizard. Built on lang-kit (`crates/lang-kit`), as each language
pack is.

It starts with Fenix. The server and the formatter are set whenever the
programs are found; the tasks and the checks are for a project with `.tf`
files. It needs Fenix 1.5 or later (extension API 1.5), and Terraform,
which it doesn't install: HashiCorp's own
([install guide](https://developer.hashicorp.com/terraform/install)),
Homebrew's, winget's, or a version manager's (`tfenv`, `tenv`) -- which
version the project wants is yours to choose. It's Terraform's: OpenTofu's
`tofu` isn't run.

## The grammar

`*.tf` files are read with tree-sitter-hcl 1.2.0's Terraform dialect,
`*.tfvars` files with the same grammar as `terraform-vars` (the name
terraform-ls knows them by), and `*.hcl` files -- `.terraform.lock.hcl`,
Terragrunt's, Packer's and Nomad's, a `.tftest.hcl`, the CLI's `.terraformrc`
-- with its HCL grammar (`grammars/`, Apache-2.0). Highlighted (a block's
type and labels, attributes, the `var.`, `local.` and `each.` roots,
functions, provider functions, templates and heredocs), folded (blocks,
objects, lists, heredocs), with Enter indenting inside a block, and the
breadcrumbs and `SPC c s` listing the blocks: `resource "aws_instance"
"web" {`. `#` comments a line. The grammar's repository has no queries; the
pack's own are in `queries/terraform/`.

## The server

terraform-ls, HashiCorp's: completion for resource, data source and
provider arguments, hover, go to definition and references, rename, and
its own problems. It's started with `terraform-ls serve` for `.tf` and
`.tfvars` files. It reads providers' schemas through Terraform, so run the
`init` task first; and it finds `terraform` on `PATH`. A Terraform the pack
found elsewhere (tfenv's folder, winget's links) is used by the tasks,
`fmt` and `validate`, but terraform-ls may not see it: put it on `PATH`.

## Formatting and checks

- `SPC c F`, and saving with `editor.format_on_save`: `terraform fmt -`,
  for `.tf`, `.tfvars` and `.hcl` files.
- On save: `terraform validate -json` on the saved `.tf` file's folder,
  as one module (`terraform.validate`), its errors and warnings shown with
  terraform-ls's, in whichever of the folder's files they are -- and in a
  module it calls, when Terraform says so. A problem about the whole
  configuration (a provider not installed, a module not installed) has no
  place: it's at the top of the file you saved. They go away on `init`
  and saving again.

  `validate` needs `terraform init` first for the providers and modules; it
  doesn't need credentials or state, and changes nothing. It checks what
  Terraform can without planning: syntax, references, the arguments a block
  takes, types. What needs a plan (a value only the cloud knows) is
  `plan`'s.

[tflint](https://github.com/terraform-linters/tflint) isn't run: it isn't
part of Terraform, and its rules are the project's to choose.

## Tasks

`SPC p t`, in this order, each in the project's root module -- the top, or
the one folder under it that holds `.tf` files (`infra/`, `terraform/`; run
there with `-chdir`):

| Task | Runs |
|---|---|
| `init` | `terraform init -input=false -no-color` |
| `validate` | `terraform validate -no-color` |
| `fmt -check` | `terraform fmt -check -diff -recursive -no-color`: the files that aren't formatted, and how they'd change |
| `plan` | `terraform plan -input=false -no-color -out=tfplan`: what would change, kept as a file |
| `apply saved plan` | `terraform apply -input=false -no-color tfplan`: **changes real infrastructure** |

A task's stdin is closed, so Terraform can't ask "Do you want to perform
these actions?", and the pack doesn't answer for you with `-auto-approve`.
`apply saved plan` applies the plan file `plan` wrote and nothing else;
Terraform refuses one that's stale (state changed since), so applying twice,
or after someone else's apply, is refused. It's the last task, it has no
key of its own, and its name says what it does: read the plan in Task
Output, then apply it. `tfplan` can hold secrets: keep it out of Git (the
project template's `.gitignore` does). There's no `destroy` task.

Every task is started by you, from the picker or `SPC p T` (rerun the
last), and only runs what's in the table. The project's `.env` goes to
each: `TF_VAR_region=eu-west-1` sets a variable, `AWS_PROFILE=orbit` a
provider's profile. A task of the same name in `.fenix/tools.json` wins.

## The doctor

| Check | | Fix row |
|---|---|---|
| Terraform | `terraform version`, 1.0 or later; needed in a project with `.tf` files, a warning in any other | none: HashiCorp, Homebrew, winget or a version manager |
| terraform-ls | `terraform-ls version`; optional | terraform-ls 0.39.1 (Linux, macOS, Windows; x86_64 and arm64) |

terraform-ls's release comes from `releases.hashicorp.com`, the file for
this machine, checked against the SHA-256 the pack knows (the same as the
release's signed `SHA256SUMS`), unpacked into the pack's tools folder, and
used at once. Starting it is said in the message line; how it ended is a
notification: "Installed terraform-ls 0.39.1", or "Couldn't install
terraform-ls 0.39.1" with which step failed and why, and *Try again*. One
you installed yourself is used instead: the pack looks on `PATH`, and in
`~/.local/bin`, `~/.tfenv/bin`, `~/.tenv/bin`, Homebrew's, Snap's, winget's
and Chocolatey's.

## Snippets

| Key | |
|---|---|
| `tf` | the `terraform` block, with a required provider |
| `prov`, `bk` | a `provider` block; a `backend` |
| `res`, `data` | a `resource`; a `data` source |
| `var`, `varv` | a variable; one with a validation |
| `out`, `loc` | an output; `locals` |
| `mod` | a module call, its source `./modules/<name>` |
| `foreach`, `count` | a resource per item (`each.key`); a resource that's on or off (`count = cond ? 1 : 0`) |
| `dyn`, `life`, `prec` | a `dynamic` block; a `lifecycle` block; a `precondition` |
| `fe`, `cond`, `tmpl` | a `for` expression; a conditional; `templatefile(...)` |
| `moved`, `imp` | a `moved` block; an `import` block |

## New projects

*Terraform · root module* in the new-project wizard: `main.tf`,
`versions.tf`, `variables.tf` and `outputs.tf`, a `.gitignore` that keeps
`.terraform/`, state and `tfplan` out of Git, and a first resource. With
the provider *none* it's Terraform's built-in `terraform_data`, so `init`,
`plan` and `apply saved plan` work with no account; *aws*, *azurerm* and
*google* add the provider's block and a data source that only reads (who
the credentials say you are). It runs nothing when it's made.

## Settings

| Setting | Default | |
|---|---|---|
| `terraform.validate` | on | `terraform validate` on a saved `.tf` file |

## Project kind

Fenix 1.6 and later know a Terraform project (`TF`): its tag in front of its name in the project hub, on Home, in the modeline and in the doctor, found by a `main.tf`, `versions.tf`, `providers.tf` or `.terraform.lock.hcl` at its top, and opened on `main.tf` when you've had none of its files open. `kind = "terraform"` in `.fenix/settings.toml` says it by hand. Earlier Fenix ignores this and the pack works there as before. The kind is declared in `extension.toml` (`[[contributes.project_kinds]]`); the pack's context markers say when the pack applies, and are not the same list.

## Keys

After `SPC m` in a Terraform project or file (`C-k m` with standard keys):
`d` checks the project again.

## What it may do

- **Run `terraform` and `terraform-ls`** (`permissions.process`): to ask
  their versions for the doctor; `terraform fmt` as the formatter,
  `terraform validate` on save, and the tasks you start; terraform-ls as
  the server. `/tools/*` is where the pack installs terraform-ls. No other
  program.
- **Reach `releases.hashicorp.com`** (`permissions.network`): to download
  terraform-ls when you run its fix row. Terraform itself reaches what its
  providers and modules need, as it does in a terminal; that's Terraform's,
  not the pack's.
- **Read the project** (`filesystem = "project-read"`): to see whether
  there are `.tf` files at the top or one folder down, and `.env`. It
  writes nothing: the plan file is Terraform's.
- **Read the file you save** (`editor = "read"`): to know which folder to
  validate.

## Not yet

- **Debugging**: Terraform has no debugger.
- **tflint**, **OpenTofu**, a `destroy` task, workspaces, and `.tf.json` and
  `.tftest.hcl` files as Terraform's (the second is read as HCL).

## Building

```text
cargo xtask pack extensions/terraform dist
cargo xtask install extensions/terraform
cargo test --locked --manifest-path extensions/terraform/Cargo.toml
```

`cargo test` runs the pack on lang-kit's fake host and the test kit's: its
tasks, server and formatter are set, the doctor reads the version of what it
finds, `terraform validate`'s JSON is read into problems, the fix row's
download names the release and its SHA-256, and a failed install is a
notification (`src/lib.rs`); each piece on its own (`src/tools.rs`,
`doctor.rs`, `tasks.rs`, `lint.rs`, `project.rs`); and the data as Fenix reads
it (`src/data.rs`) -- the language files, the grammars loaded and the
queries run on a sample, the snippets, the project template, and the
manifest against the code. What `terraform validate` and `terraform version`
printed in `src/samples/` was recorded from Terraform 1.17.2 and
terraform-ls 0.39.1 (see `src/samples/README.md`).
