terraform {
  required_version = ">= 1.5"
{{#if provider == aws}}

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
{{/if}}
{{#if provider == azurerm}}

  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = ">= 4.0"
    }
  }
{{/if}}
{{#if provider == google}}

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 6.0"
    }
  }
{{/if}}
}
