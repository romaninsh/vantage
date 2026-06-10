# LICENSE carries someone else's copyright; Apache-2.0 file missing despite dual-license claim

- **Severity:** high
- **Category:** bugs
- **Location:** `LICENSE:3`

The repo's only license file is MIT with "Copyright (c) 2015 Sean Griffin" — Sean Griffin is the Diesel author, so this file appears copied verbatim from another project and assigns copyright to the wrong person/year. Meanwhile all 21 publishable crates declare `license = "MIT OR Apache-2.0"` in Cargo.toml, but no Apache-2.0 license text exists anywhere in the repo, and no per-crate license files are packaged. The website (vantage-web2/positioning.md) leans on "open-source, MIT-licensed Vantage Framework" as a trust signal, so a wrong copyright holder directly undermines a headline claim.

```
The MIT License (MIT)

Copyright (c) 2015 Sean Griffin
```
```
# */Cargo.toml (21 crates)
license = "MIT OR Apache-2.0"
```

**Recommendation:** Replace the copyright line with the actual author (Romans Malinovskis) and current year, add a LICENSE-APACHE file (or change crate metadata to `license = "MIT"` to match the website), and ensure license files are included in published packages.
