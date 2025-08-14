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
  value = aws_lb_target_group.billing.arn
}

output "payments_target_group_arn" {
  value = aws_lb_target_group.payments.arn
}

output "listener_arn" {
  value = var.certificate_arn != null ? aws_lb_listener.https[0].arn : aws_lb_listener.http.arn
}