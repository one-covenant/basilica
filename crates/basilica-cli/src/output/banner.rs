//! ASCII art banners and welcome messages

use console::style;

/// Display the main Basilica ASCII art banner
pub fn print_welcome_banner() {
    let banner = r#"
 ██████╗  █████╗ ███████╗██╗██╗     ██╗ ██████╗ █████╗ 
 ██╔══██╗██╔══██╗██╔════╝██║██║     ██║██╔════╝██╔══██╗
 ██████╔╝███████║███████╗██║██║     ██║██║     ███████║
 ██╔══██╗██╔══██║╚════██║██║██║     ██║██║     ██╔══██║
 ██████╔╝██║  ██║███████║██║███████╗██║╚██████╗██║  ██║
 ╚═════╝ ╚═╝  ╚═╝╚══════╝╚═╝╚══════╝╚═╝ ╚═════╝╚═╝  ╚═╝
              GPU Marketplace for AI/ML
 "#;

    // Print banner in green
    for line in banner.lines() {
        println!("{}", style(line).red().bright());
    }
}

/// Display post-login welcome message with helpful commands
pub fn print_post_login_welcome() {
    println!(
        "{}",
        style("First time setup detected. Let's get you authenticated.").dim()
    );
    println!();

    print_command_suggestions();
}

/// Print helpful command suggestions
pub fn print_command_suggestions() {
    println!("{}", style("Quick Commands:").cyan().bold());
    println!();

    // List available GPUs
    println!(
        "  {} {}",
        style("basilica ls").yellow().bold(),
        style("- View available GPUs for rental").dim()
    );

    // Start a rental
    println!(
        "  {} {}",
        style("basilica up").yellow().bold(),
        style("- Start a GPU rental session").dim()
    );

    // Check status
    println!(
        "  {} {}",
        style("basilica status").yellow().bold(),
        style("- Check status of your current rentals").dim()
    );

    // SSH into rental
    println!(
        "  {} {}",
        style("basilica ssh").yellow().bold(),
        style("- Connect to your rented GPU").dim()
    );

    // Stop a rental
    println!(
        "  {} {}",
        style("basilica down").yellow().bold(),
        style("- Stop a GPU rental").dim()
    );

    println!();
    println!(
        "For more information, run {}",
        style("basilica --help").green(),
    );
}
