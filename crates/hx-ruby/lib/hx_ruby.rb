# frozen_string_literal: true

require_relative "hx_ruby/version"

# Load the compiled Rust extension. rake-compiler installs it under a Ruby
# version directory for gems that ship several; a plain extconf build (what
# Bundler runs for a path gem) drops it straight in lib/hx_ruby. Try the
# versioned path first, then fall back.
begin
  ruby_version = RUBY_VERSION[/\d+\.\d+/]
  require_relative "hx_ruby/#{ruby_version}/hx_ruby"
rescue LoadError
  begin
    require_relative "hx_ruby/hx_ruby"
  rescue LoadError => e
    # Bundler does not compile a path gem's extension on install, so a fresh
    # checkout has to build it once.
    raise LoadError, "#{e.message}\n\nThe hx_ruby native extension is not " \
      "built yet. From the gem directory run: bundle exec rake compile"
  end
end

# Ruby bindings over hx-catalog's `.hlx` inspector.
#
# The native `HxRuby.inspect_hlx(json)` is defined in Rust (see ext/hx_ruby).
# It takes a preset's JSON string and returns a Hash of tone facts, raising
# {HxRuby::CatalogNotInstalled} when the HX Edit resources are missing and
# {HxRuby::Error} for any other catalog read failure.
module HxRuby
end
