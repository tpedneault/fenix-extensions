variable "name" {
  description = "What this configuration is called, for names and tags."
  type        = string
  default     = "{{name_kebab}}"
}
{{#if provider == aws}}

variable "region" {
  description = "The AWS region to work in."
  type        = string
  default     = "eu-west-1"
}
{{/if}}
{{#if provider == google}}

variable "region" {
  description = "The Google Cloud region to work in."
  type        = string
  default     = "europe-west1"
}
{{/if}}
