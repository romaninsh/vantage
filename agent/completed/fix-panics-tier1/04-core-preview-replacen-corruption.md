# Expression::preview() corrupts output when parameter values contain `{}`

- **Severity:** low
- **Category:** bugs
- **Location:** `vantage-expressions/src/expression/core.rs:168-175`

`preview()` substitutes parameters with `replacen("{}", ..., 1)` sequentially. If an earlier parameter's rendered value itself contains `{}` (a JSON object/string like `"x{}y"` or `{}`), subsequent replacements target the `{}` *inside the already-substituted value* instead of the next template placeholder, producing a scrambled preview. `preview()` is also the `Debug` impl and is what `MockBuilder::on_exact_select` matches against, so mocks and error messages can mis-resolve for such values.

```rust
pub fn preview(&self) -> String {
    let mut preview = self.template.clone();
    for param in &self.parameters {
        let param_str = param.preview();
        preview = preview.replacen("{}", &param_str, 1);
    }
    preview
}
```

**Recommendation:** Build the preview in one pass by splitting the template on `{}` and interleaving parameter strings (as `flatten_nested` does), so substituted values are never re-scanned.
