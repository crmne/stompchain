# Asking crates.io whether a version is published.
#
# Sourced by the release workflow rather than written out twice, because the two
# places that ask - "has this already gone out?" before publishing, and "did it
# arrive?" after - have to agree, and the first time they did not agree they
# published two crates and then reported both missing.
#
# The header is the whole point. crates.io answers 403 to a request with no
# User-Agent, and `curl -sSf` turned that into a non-zero exit that read exactly
# like "not published". So the pre-check never found anything already published
# and the post-check never found anything at all.

# published <crate> <version>
published() {
    local crate="$1" version="$2"
    curl -sSf \
        -H "User-Agent: tonepush release (https://github.com/crmne/tonepush)" \
        "https://crates.io/api/v1/crates/${crate}/${version}" \
        >/dev/null 2>&1
}
