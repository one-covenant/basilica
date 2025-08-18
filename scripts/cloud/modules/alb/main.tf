data "aws_region" "current" {}

resource "aws_lb" "main" {
  name               = "${var.name_prefix}-alb"
  internal           = false
  load_balancer_type = "application"
  security_groups    = var.security_group_ids
  subnets            = var.subnet_ids

  enable_deletion_protection = false

  tags = var.tags
}

resource "aws_lb_target_group" "billing" {
  name        = "${var.name_prefix}-billing-http-tg"
  port        = 8080
  protocol    = "HTTP"
  vpc_id      = var.vpc_id
  target_type = "ip"

  health_check {
    enabled             = true
    healthy_threshold   = 2
    unhealthy_threshold = 3
    timeout             = 10
    interval            = 30
    port                = "traffic-port"
    protocol            = "HTTP"
    path                = "/health"
    matcher             = "200"
  }

  deregistration_delay = 30

  tags = var.tags
}

resource "aws_lb_target_group" "payments" {
  name        = "${var.name_prefix}-payments-tg"
  port        = 8082
  protocol    = "HTTP"
  vpc_id      = var.vpc_id
  target_type = "ip"

  health_check {
    enabled             = true
    healthy_threshold   = 2
    unhealthy_threshold = 2
    timeout             = 5
    interval            = 30
    path                = "/health"
    matcher             = "200"
    port                = "traffic-port"
    protocol            = "HTTP"
  }

  deregistration_delay = 30

  tags = var.tags
}

resource "aws_lb_target_group" "default" {
  name        = "${var.name_prefix}-default-tg"
  port        = 80
  protocol    = "HTTP"
  vpc_id      = var.vpc_id
  target_type = "ip"

  health_check {
    enabled             = true
    healthy_threshold   = 2
    unhealthy_threshold = 2
    timeout             = 5
    interval            = 30
    path                = "/"
    matcher             = "404"
    port                = "traffic-port"
    protocol            = "HTTP"
  }

  tags = var.tags
}

resource "aws_lb_target_group" "additional" {
  for_each = var.additional_target_groups

  name             = "${var.name_prefix}-${each.key}"
  port             = each.value.port
  protocol         = each.value.protocol
  protocol_version = each.value.protocol_version
  vpc_id           = var.vpc_id
  target_type      = "ip"

  health_check {
    enabled             = true
    healthy_threshold   = each.value.health_check.healthy_threshold
    unhealthy_threshold = each.value.health_check.unhealthy_threshold
    timeout             = each.value.health_check.timeout
    interval            = each.value.health_check.interval
    port                = "traffic-port"
    protocol            = each.value.health_check.protocol
    path                = each.value.health_check.path
    matcher             = each.value.health_check.matcher
  }

  deregistration_delay = 30

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-${each.key}"
  })
}

resource "aws_lb_listener" "http" {
  load_balancer_arn = aws_lb.main.arn
  port              = "80"
  protocol          = "HTTP"

  default_action {
    type = var.certificate_arn != null ? "redirect" : "fixed_response"

    dynamic "redirect" {
      for_each = var.certificate_arn != null ? [1] : []
      content {
        port        = "443"
        protocol    = "HTTPS"
        status_code = "HTTP_301"
      }
    }

    dynamic "fixed_response" {
      for_each = var.certificate_arn != null ? [] : [1]
      content {
        content_type = "text/plain"
        message_body = "Service Unavailable"
        status_code  = "503"
      }
    }
  }

  tags = var.tags
}

resource "aws_lb_listener" "https" {
  count             = var.certificate_arn != null ? 1 : 0
  load_balancer_arn = aws_lb.main.arn
  port              = "443"
  protocol          = "HTTPS"
  ssl_policy        = "ELBSecurityPolicy-TLS13-1-2-2021-06"
  certificate_arn   = var.certificate_arn

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.default.arn
  }

  tags = var.tags
}

resource "aws_lb_listener_rule" "billing" {
  listener_arn = var.certificate_arn != null ? aws_lb_listener.https[0].arn : aws_lb_listener.http.arn
  priority     = 100

  action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.billing.arn
  }

  condition {
    path_pattern {
      values = ["/billing.*", "/api.billing.*", "/grpc.health.v1.Health/*"]
    }
  }
}

resource "aws_lb_listener_rule" "payments" {
  listener_arn = var.certificate_arn != null ? aws_lb_listener.https[0].arn : aws_lb_listener.http.arn
  priority     = 200

  action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.payments.arn
  }

  condition {
    path_pattern {
      values = ["/payments/*", "/api/payments/*"]
    }
  }
}

resource "aws_lb_listener_rule" "additional" {
  for_each = {
    for k, v in var.additional_listener_rules : k => v
    if v.listener_protocol != "HTTPS" || var.certificate_arn != null
  }

  listener_arn = each.value.listener_protocol == "HTTPS" ? aws_lb_listener.https[0].arn : aws_lb_listener.http.arn
  priority     = each.value.priority

  action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.additional[each.value.target_group_key].arn
  }

  condition {
    path_pattern {
      values = each.value.path_patterns
    }
  }
}
