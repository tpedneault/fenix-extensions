# The root module: what this configuration builds. Its providers and
# Terraform's version are in versions.tf, its inputs in variables.tf and
# what it tells you in outputs.tf.
#
#   terraform init    SPC p t, init: gets the providers and modules
#   terraform plan    SPC p t, plan: what would change, written to tfplan
#   terraform apply   SPC p t, apply saved plan: does exactly that plan
{{#if provider == none}}

resource "terraform_data" "greeting" {
  input = "Hello from ${var.name}"
}
{{/if}}
{{#if provider == aws}}

provider "aws" {
  region = var.region
}

# Read-only: who the credentials say you are.
data "aws_caller_identity" "current" {}
{{/if}}
{{#if provider == azurerm}}

provider "azurerm" {
  features {}
}

# Read-only: which subscription the credentials say you are in.
data "azurerm_client_config" "current" {}
{{/if}}
{{#if provider == google}}

provider "google" {
  region = var.region
}

# Read-only: which project the credentials say you are in.
data "google_client_config" "current" {}
{{/if}}
