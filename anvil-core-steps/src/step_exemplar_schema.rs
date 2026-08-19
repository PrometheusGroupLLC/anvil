//! Step module for step_exemplar_schema.feature.

use anvil_core::domain::playbook::exemplar_step::{
    load_step_exemplar_from_markdown, StepExemplar, StepExemplarLoadError,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const SEX_MARKDOWN_KEY: &str = "step_exemplar_markdown";
const SEX_LOAD_KEY: &str = "step_exemplar_load_result";

fn step_exemplar_markdown(grain: &str, band: &str, source_instance: Option<&str>) -> String {
    let mut frontmatter = format!("grain: {grain}\nstate: spec\nband: {band}\n");
    if let Some(source_instance) = source_instance {
        frontmatter.push_str(&format!("source_instance: {source_instance}\n"));
    }
    if band == "ceiling" {
        frontmatter.push_str("synthetic: true\n");
    }
    format!(
        "---\n{frontmatter}---\nThis distilled pattern preserves the useful structure without raw material.\n"
    )
}

fn step_exemplar_markdown_for_band(band: &str) -> String {
    let source_instance = if band == "ceiling" {
        None
    } else {
        Some("hearth/tracks/20260101T0000_example")
    };
    step_exemplar_markdown("step", band, source_instance)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a step exemplar Markdown file with band {string}",
            &[],
            &[(SEX_MARKDOWN_KEY, "String")],
            |_ctx, params| {
                let band = params.get_string(0).ok_or("Expected band")?;
                let mut out = Context::new();
                out.set(SEX_MARKDOWN_KEY, step_exemplar_markdown_for_band(&band));
                Ok(out)
            },
        ),
        step_def(
            "a step exemplar Markdown file with grain {string}",
            &[],
            &[(SEX_MARKDOWN_KEY, "String")],
            |_ctx, params| {
                let grain = params.get_string(0).ok_or("Expected grain")?;
                let mut out = Context::new();
                out.set(
                    SEX_MARKDOWN_KEY,
                    step_exemplar_markdown(&grain, "good", Some("hearth/tracks/20260101T0000_example")),
                );
                Ok(out)
            },
        ),
        step_def(
            "a step exemplar Markdown file with band {string} and no source_instance",
            &[],
            &[(SEX_MARKDOWN_KEY, "String")],
            |_ctx, params| {
                let band = params.get_string(0).ok_or("Expected band")?;
                let mut out = Context::new();
                out.set(SEX_MARKDOWN_KEY, step_exemplar_markdown("step", &band, None));
                Ok(out)
            },
        ),
        step_def(
            "a step exemplar Markdown file with no frontmatter",
            &[],
            &[(SEX_MARKDOWN_KEY, "String")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(
                    SEX_MARKDOWN_KEY,
                    "This artifact has no YAML frontmatter at all.\n".to_string(),
                );
                Ok(out)
            },
        ),
        step_def(
            "a step exemplar Markdown file containing raw marker {string}",
            &[],
            &[(SEX_MARKDOWN_KEY, "String")],
            |_ctx, params| {
                let marker = params.get_string(0).ok_or("Expected raw marker")?;
                let markdown = if marker == "body_fence" {
                    step_exemplar_markdown_for_band("good")
                        .replace("This distilled pattern", "```raw-artifact\nsecret\n```\nThis distilled pattern")
                } else {
                    step_exemplar_markdown_for_band("good")
                        .replace("band:", &format!("{}: forbidden\nband:", marker))
                };
                let mut out = Context::new();
                out.set(SEX_MARKDOWN_KEY, markdown);
                Ok(out)
            },
        ),
        step_def(
            "the step exemplar is loaded",
            &[(SEX_MARKDOWN_KEY, "String")],
            &[(SEX_LOAD_KEY, "Result<StepExemplar, StepExemplarLoadError>")],
            |ctx, _params| {
                let markdown = ctx.get::<String>(SEX_MARKDOWN_KEY).ok_or("No step exemplar markdown")?;
                let mut out = Context::new();
                out.set(SEX_LOAD_KEY, load_step_exemplar_from_markdown(markdown));
                Ok(out)
            },
        ),
        check_def(
            "the step exemplar load succeeds",
            &[(SEX_LOAD_KEY, "Result<StepExemplar, StepExemplarLoadError>")],
            |ctx, _params| match ctx
                .get::<Result<StepExemplar, StepExemplarLoadError>>(SEX_LOAD_KEY)
                .ok_or("No step exemplar load result")?
            {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Expected step exemplar load success, got {}", e)),
            },
        ),
        check_def(
            "the loaded step exemplar band is {string}",
            &[(SEX_LOAD_KEY, "Result<StepExemplar, StepExemplarLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected band")?;
                let exemplar = ctx
                    .get::<Result<StepExemplar, StepExemplarLoadError>>(SEX_LOAD_KEY)
                    .ok_or("No step exemplar load result")?
                    .as_ref()
                    .map_err(|e| format!("Expected step exemplar load success, got {}", e))?;
                if exemplar.frontmatter.band == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected band '{}' got '{}'",
                        expected, exemplar.frontmatter.band
                    ))
                }
            },
        ),
        check_def(
            "the loaded step exemplar body contains {string}",
            &[(SEX_LOAD_KEY, "Result<StepExemplar, StepExemplarLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected body text")?;
                let exemplar = ctx
                    .get::<Result<StepExemplar, StepExemplarLoadError>>(SEX_LOAD_KEY)
                    .ok_or("No step exemplar load result")?
                    .as_ref()
                    .map_err(|e| format!("Expected step exemplar load success, got {}", e))?;
                if exemplar.body.contains(&expected) {
                    Ok(())
                } else {
                    Err(format!("Body did not contain '{}': {}", expected, exemplar.body))
                }
            },
        ),
        check_def(
            "the step exemplar load fails with code {string}",
            &[(SEX_LOAD_KEY, "Result<StepExemplar, StepExemplarLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected error code")?;
                match ctx
                    .get::<Result<StepExemplar, StepExemplarLoadError>>(SEX_LOAD_KEY)
                    .ok_or("No step exemplar load result")?
                {
                    Ok(exemplar) => Err(format!("Expected step exemplar load failure, got {:?}", exemplar)),
                    Err(e) if e.code() == expected => Ok(()),
                    Err(e) => Err(format!("Expected code '{}' got '{}': {}", expected, e.code(), e)),
                }
            },
        ),
    ]
}
