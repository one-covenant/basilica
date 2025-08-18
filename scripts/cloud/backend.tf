terraform {
  backend "s3" {
    bucket         = "basilica-terraform-state"
    key            = "terraform.tfstate"
    region         = "us-east-2"
    encrypt        = true
    dynamodb_table = "basilica-terraform-locks"

    skip_credentials_validation = false
    skip_metadata_api_check     = false
    skip_region_validation      = false
    force_path_style            = false
  }
}
