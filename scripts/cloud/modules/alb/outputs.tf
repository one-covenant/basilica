output "alb_arn" {
  value = aws_lb.main.arn
}

output "alb_dns_name" {
  value = aws_lb.main.dns_name
}

output "alb_zone_id" {
  value = aws_lb.main.zone_id
}

output "billing_target_group_arn" {
  value = var.create_billing_target_group ? aws_lb_target_group.billing[0].arn : null
}

output "payments_target_group_arn" {
  value = var.create_payments_target_group ? aws_lb_target_group.payments[0].arn : null
}

output "default_target_group_arn" {
  value = aws_lb_target_group.default.arn
}

output "additional_target_group_arns" {
  value = { for k, v in aws_lb_target_group.additional : k => v.arn }
}

output "listener_arn" {
  value = var.certificate_arn != null ? aws_lb_listener.https[0].arn : aws_lb_listener.http.arn
}
