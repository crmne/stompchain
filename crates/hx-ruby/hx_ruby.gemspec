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
  spec.homepage = "https://github.com/crmne/tonepush"
  spec.license = "MIT"
  spec.required_ruby_version = ">= 3.1.0"

  spec.metadata["homepage_uri"] = spec.homepage
  spec.metadata["source_code_uri"] = spec.homepage
  spec.metadata["rubygems_mfa_required"] = "true"

  # The sibling crates this extension depends on live in the Cargo workspace, so
  # this gem is meant to be consumed by path, not built and pushed on its own.
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
