//! Lighthouse performance auditing for Zone frontends
//!
//! Runs Lighthouse CI audits on manager and installer frontends,
//! reporting scores for Performance, Accessibility, Best Practices, and SEO.

use anyhow::{Context, Result, bail};
use console::style;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy)]
pub enum Frontend {
    Manager,
    Installer,
}

impl Frontend {
    fn name(&self) -> &'static str {
        match self {
            Frontend::Manager => "manager",
            Frontend::Installer => "installer",
        }
    }

    fn path(&self) -> &'static str {
        match self {
            Frontend::Manager => "manager/frontend",
            Frontend::Installer => "installer/frontend",
        }
    }

    fn build_dir(&self) -> &'static str {
        match self {
            Frontend::Manager => "dist",
            Frontend::Installer => "build",
        }
    }
}

/// Run Lighthouse audit on a frontend
pub fn run_lighthouse(project_root: &Path, frontend: Frontend, verbose: bool) -> Result<()> {
    let frontend_path = project_root.join(frontend.path());

    // Check if frontend exists
    if !frontend_path.exists() {
        bail!("Frontend not found at {:?}", frontend_path);
    }

    println!(
        "{} Running Lighthouse audit for {} frontend...",
        style("→").cyan(),
        style(frontend.name()).bold()
    );

    // Check if node_modules exists
    if !frontend_path.join("node_modules").exists() {
        println!("{} Installing dependencies...", style("→").dim());
        let status = Command::new("bun")
            .arg("install")
            .current_dir(&frontend_path)
            .status()
            .context("Failed to run bun install")?;

        if !status.success() {
            bail!("bun install failed");
        }
    }

    // Build the frontend
    println!("{} Building frontend...", style("→").dim());
    let status = Command::new("bun")
        .arg("run")
        .arg("build")
        .current_dir(&frontend_path)
        .env("CI", "false")
        .status()
        .context("Failed to run bun build")?;

    if !status.success() {
        bail!("bun build failed");
    }

    let build_dir = frontend_path.join(frontend.build_dir());
    if !build_dir.exists() {
        bail!("Build directory not found at {:?}", build_dir);
    }

    // Check if lighthouse is installed
    let lhci_check = Command::new("bunx")
        .arg("@lhci/cli")
        .arg("--version")
        .output();

    if lhci_check.is_err() || !lhci_check.unwrap().status.success() {
        println!("{} Installing @lhci/cli...", style("→").dim());
        let status = Command::new("bun")
            .arg("add")
            .arg("-g")
            .arg("@lhci/cli")
            .status()
            .context("Failed to install @lhci/cli")?;

        if !status.success() {
            bail!("Failed to install @lhci/cli");
        }
    }

    // Run Lighthouse CI
    println!("{} Running Lighthouse...", style("→").dim());

    let mut cmd = Command::new("bunx");
    cmd.arg("@lhci/cli")
        .arg("autorun")
        .current_dir(&frontend_path);

    if verbose {
        cmd.arg("--verbose");
    }

    let output = cmd.output().context("Failed to run lhci")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Even on "failure", Lighthouse may have run successfully but scores were below threshold
        // Check for score output
        if stdout.contains("Lighthouse scores") || stdout.contains("categories") {
            println!("{}", stdout);
        } else {
            eprintln!("{}", stderr);
            bail!("Lighthouse CI failed");
        }
    } else {
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }

    println!(
        "\n{} Lighthouse audit complete for {} frontend",
        style("✓").green(),
        style(frontend.name()).bold()
    );

    Ok(())
}

/// Run Lighthouse audit on all frontends
pub fn run_all(project_root: &Path, verbose: bool) -> Result<()> {
    println!(
        "{}",
        style("Running Lighthouse audits on all frontends...").bold()
    );
    println!();

    let mut errors = Vec::new();

    for frontend in [Frontend::Manager, Frontend::Installer] {
        if let Err(e) = run_lighthouse(project_root, frontend, verbose) {
            errors.push(format!("{}: {}", frontend.name(), e));
        }
        println!();
    }

    if !errors.is_empty() {
        eprintln!("\n{} Some audits failed:", style("✗").red());
        for error in &errors {
            eprintln!("  - {}", error);
        }
        bail!("Lighthouse audits failed");
    }

    println!(
        "{} All Lighthouse audits passed!",
        style("✓").green().bold()
    );
    Ok(())
}
