{{#if provider == none}}
output "greeting" {
  description = "What the terraform_data resource was given."
  value       = terraform_data.greeting.output
}
{{/if}}
{{#if provider == aws}}
output "account_id" {
  description = "The AWS account the credentials are for."
  value       = data.aws_caller_identity.current.account_id
}
{{/if}}
{{#if provider == azurerm}}
output "subscription_id" {
  description = "The Azure subscription the credentials are for."
  value       = data.azurerm_client_config.current.subscription_id
}
{{/if}}
{{#if provider == google}}
output "project" {
  description = "The Google Cloud project the credentials are for."
  value       = data.google_client_config.current.project
}
{{/if}}
