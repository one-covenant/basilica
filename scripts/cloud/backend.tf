terraform {
  backend "s3" {
    bucket         = "basilica-terraform-state"
    key            = "terraform.tfstate"
    region         = "us-east-1"
    encrypt        = true
    dynamodb_table = "basilica-terraform-locks"
  }
}