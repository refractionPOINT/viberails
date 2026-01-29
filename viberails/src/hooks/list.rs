use std::env;

use anyhow::{Context, Result};

use crate::{
    common::print_header,
    providers::{Claude, LLmProviderTrait},
};

////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////

pub fn list() -> Result<()> {
    let mut prov: Vec<Box<dyn LLmProviderTrait>> = vec![];

    let current_exe = env::current_exe().context("Unable to find current exe")?;

    if let Ok(claude) = Claude::new(current_exe) {
        prov.push(Box::new(claude));
    }

    print_header();

    for p in prov {
        println!("\nProvider: {}", &p.name());
        if let Ok(hooks) = p.list() {
            for hook in hooks {
                println!(
                    "    {:<20} {:<10} {}",
                    hook.hook_type, hook.matcher, hook.command
                );
            }
        }
    }

    Ok(())
}
