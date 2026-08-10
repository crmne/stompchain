# frozen_string_literal: true

require_relative "lib/hx_ruby/version"

Gem::Specification.new do |spec|
  spec.name = "hx_ruby"
  spec.version = HxRuby::VERSION
  spec.authors = ["Carmine Paolino"]
  spec.email = ["carmine@paolino.me"]

  spec.summary = "Parse Line 6 .hlx presets into tone facts with the TonePush Rust catalog"
  spec.description = <<~DESC.tr("\n", " ").strip
    Ruby bindings over hx-catalog's preset inspector, so the tone browser reads
    .hlx files with the same Rust code the desktop uses instead of
    reimplementing parsing in Ruby.
  DESC
  spec.homepage = "https://docs.tonepush.rocks"
  spec.license = "MIT"
  spec.required_ruby_version = ">= 3.1.0"

  spec.metadata["homepage_uri"] = spec.homepage
  spec.metadata["source_code_uri"] = "https://github.com/crmne/tonepush"
  spec.metadata["changelog_uri"] = "https://github.com/crmne/tonepush/releases"
  spec.metadata["rubygems_mfa_required"] = "true"

  # Sources only. The extension is compiled on the installing machine, which
  # needs a Rust toolchain; the crates it builds against come from crates.io,
  # which is why ext/hx_ruby/Cargo.toml names them by version and not by path.
  spec.files = Dir[
    "lib/**/*.rb",
    "ext/hx_ruby/extconf.rb",
    "ext/hx_ruby/Cargo.toml",
    "ext/hx_ruby/src/**/*.rs"
  ]
  spec.require_paths = ["lib"]
  spec.extensions = ["ext/hx_ruby/extconf.rb"]

  spec.add_dependency "rb_sys", "~> 0.9"
end
