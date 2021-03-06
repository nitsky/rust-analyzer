//! `completions` crate provides utilities for generating completions of user input.

mod config;
mod item;
mod context;
mod patterns;
mod generated_lint_completions;
#[cfg(test)]
mod test_utils;
mod render;

mod completions;

use ide_db::base_db::FilePosition;
use ide_db::RootDatabase;

use crate::{completions::Completions, context::CompletionContext, item::CompletionKind};

pub use crate::{
    config::CompletionConfig,
    item::{CompletionItem, CompletionItemKind, CompletionScore, InsertTextFormat},
};

//FIXME: split the following feature into fine-grained features.

// Feature: Magic Completions
//
// In addition to usual reference completion, rust-analyzer provides some ✨magic✨
// completions as well:
//
// Keywords like `if`, `else` `while`, `loop` are completed with braces, and cursor
// is placed at the appropriate position. Even though `if` is easy to type, you
// still want to complete it, to get ` { }` for free! `return` is inserted with a
// space or `;` depending on the return type of the function.
//
// When completing a function call, `()` are automatically inserted. If a function
// takes arguments, the cursor is positioned inside the parenthesis.
//
// There are postfix completions, which can be triggered by typing something like
// `foo().if`. The word after `.` determines postfix completion. Possible variants are:
//
// - `expr.if` -> `if expr {}` or `if let ... {}` for `Option` or `Result`
// - `expr.match` -> `match expr {}`
// - `expr.while` -> `while expr {}` or `while let ... {}` for `Option` or `Result`
// - `expr.ref` -> `&expr`
// - `expr.refm` -> `&mut expr`
// - `expr.let` -> `let <|> = expr;`
// - `expr.letm` -> `let mut <|> = expr;`
// - `expr.not` -> `!expr`
// - `expr.dbg` -> `dbg!(expr)`
// - `expr.dbgr` -> `dbg!(&expr)`
// - `expr.call` -> `(expr)`
//
// There also snippet completions:
//
// .Expressions
// - `pd` -> `eprintln!(" = {:?}", );`
// - `ppd` -> `eprintln!(" = {:#?}", );`
//
// .Items
// - `tfn` -> `#[test] fn feature(){}`
// - `tmod` ->
// ```rust
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_name() {}
// }
// ```
//
// And experimental completions, enabled with the `rust-analyzer.completion.enableExperimental` setting.
// This flag enables or disables:
//
// - Auto import: additional completion options with automatic `use` import and options from all project importable items, matched for the input
//
// Experimental completions might cause issues with performance and completion list look.

/// Main entry point for completion. We run completion as a two-phase process.
///
/// First, we look at the position and collect a so-called `CompletionContext.
/// This is a somewhat messy process, because, during completion, syntax tree is
/// incomplete and can look really weird.
///
/// Once the context is collected, we run a series of completion routines which
/// look at the context and produce completion items. One subtlety about this
/// phase is that completion engine should not filter by the substring which is
/// already present, it should give all possible variants for the identifier at
/// the caret. In other words, for
///
/// ```no_run
/// fn f() {
///     let foo = 92;
///     let _ = bar<|>
/// }
/// ```
///
/// `foo` *should* be present among the completion variants. Filtering by
/// identifier prefix/fuzzy match should be done higher in the stack, together
/// with ordering of completions (currently this is done by the client).
pub fn completions(
    db: &RootDatabase,
    config: &CompletionConfig,
    position: FilePosition,
) -> Option<Completions> {
    let ctx = CompletionContext::new(db, position, config)?;

    if ctx.no_completion_required() {
        // No work required here.
        return None;
    }

    let mut acc = Completions::default();
    completions::attribute::complete_attribute(&mut acc, &ctx);
    completions::fn_param::complete_fn_param(&mut acc, &ctx);
    completions::keyword::complete_expr_keyword(&mut acc, &ctx);
    completions::keyword::complete_use_tree_keyword(&mut acc, &ctx);
    completions::snippet::complete_expr_snippet(&mut acc, &ctx);
    completions::snippet::complete_item_snippet(&mut acc, &ctx);
    completions::qualified_path::complete_qualified_path(&mut acc, &ctx);
    completions::unqualified_path::complete_unqualified_path(&mut acc, &ctx);
    completions::dot::complete_dot(&mut acc, &ctx);
    completions::record::complete_record(&mut acc, &ctx);
    completions::pattern::complete_pattern(&mut acc, &ctx);
    completions::postfix::complete_postfix(&mut acc, &ctx);
    completions::macro_in_item_position::complete_macro_in_item_position(&mut acc, &ctx);
    completions::trait_impl::complete_trait_impl(&mut acc, &ctx);
    completions::mod_::complete_mod(&mut acc, &ctx);

    Some(acc)
}

#[cfg(test)]
mod tests {
    use crate::config::CompletionConfig;
    use crate::test_utils;

    struct DetailAndDocumentation<'a> {
        detail: &'a str,
        documentation: &'a str,
    }

    fn check_detail_and_documentation(ra_fixture: &str, expected: DetailAndDocumentation) {
        let (db, position) = test_utils::position(ra_fixture);
        let config = CompletionConfig::default();
        let completions: Vec<_> = crate::completions(&db, &config, position).unwrap().into();
        for item in completions {
            if item.detail() == Some(expected.detail) {
                let opt = item.documentation();
                let doc = opt.as_ref().map(|it| it.as_str());
                assert_eq!(doc, Some(expected.documentation));
                return;
            }
        }
        panic!("completion detail not found: {}", expected.detail)
    }

    fn check_no_completion(ra_fixture: &str) {
        let (db, position) = test_utils::position(ra_fixture);
        let config = CompletionConfig::default();

        let completions: Option<Vec<String>> = crate::completions(&db, &config, position)
            .and_then(|completions| {
                let completions: Vec<_> = completions.into();
                if completions.is_empty() {
                    None
                } else {
                    Some(completions)
                }
            })
            .map(|completions| {
                completions.into_iter().map(|completion| format!("{:?}", completion)).collect()
            });

        // `assert_eq` instead of `assert!(completions.is_none())` to get the list of completions if test will panic.
        assert_eq!(completions, None, "Completions were generated, but weren't expected");
    }

    #[test]
    fn test_completion_detail_from_macro_generated_struct_fn_doc_attr() {
        check_detail_and_documentation(
            r#"
            //- /lib.rs
            macro_rules! bar {
                () => {
                    struct Bar;
                    impl Bar {
                        #[doc = "Do the foo"]
                        fn foo(&self) {}
                    }
                }
            }

            bar!();

            fn foo() {
                let bar = Bar;
                bar.fo<|>;
            }
            "#,
            DetailAndDocumentation { detail: "fn foo(&self)", documentation: "Do the foo" },
        );
    }

    #[test]
    fn test_completion_detail_from_macro_generated_struct_fn_doc_comment() {
        check_detail_and_documentation(
            r#"
            //- /lib.rs
            macro_rules! bar {
                () => {
                    struct Bar;
                    impl Bar {
                        /// Do the foo
                        fn foo(&self) {}
                    }
                }
            }

            bar!();

            fn foo() {
                let bar = Bar;
                bar.fo<|>;
            }
            "#,
            DetailAndDocumentation { detail: "fn foo(&self)", documentation: " Do the foo" },
        );
    }

    #[test]
    fn test_no_completions_required() {
        // There must be no hint for 'in' keyword.
        check_no_completion(
            r#"
            fn foo() {
                for i i<|>
            }
            "#,
        );
        // After 'in' keyword hints may be spawned.
        check_detail_and_documentation(
            r#"
            /// Do the foo
            fn foo() -> &'static str { "foo" }

            fn bar() {
                for c in fo<|>
            }
            "#,
            DetailAndDocumentation {
                detail: "fn foo() -> &'static str",
                documentation: "Do the foo",
            },
        );
    }
}
