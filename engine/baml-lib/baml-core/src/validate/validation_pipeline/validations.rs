mod classes;
mod clients;
mod configurations;
mod cycle;
mod enums;
mod functions;
mod template_strings;
mod types;

use super::context::Context;

pub(super) fn validate(ctx: &mut Context<'_>) {
    enums::validate(ctx);
    classes::validate(ctx);
    functions::validate(ctx);
    clients::validate(ctx);
    template_strings::validate(ctx);
    configurations::validate(ctx);

    if !ctx.diagnostics.has_errors() {
        cycle::validate(ctx);
    }
}
