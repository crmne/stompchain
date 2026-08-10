# frozen_string_literal: true

require_relative "lib/hx_ruby/version"

Gem::Specification.new do |spec|
  spec.name = "hx_ruby"
  spec.version = HxRuby::VERSION
  spec.authors = ["Carmine Paolino"]
  spec.email = ["carmine@paolino.me"]

  # Written for somebody who has never heard of this project. They arrive from a
  # search, and the first sentence has to say what the thing is without leaning
  # on the name of any other part of it.
  spec.summary = "Read Line 6 Helix and HX guitar preset files from Ruby"
  spec.description = <<~DESC.tr("\n", " ").strip
    A preset on a Line 6 Helix or HX Stomp is a saved guitar rig: an amp, a
    speaker cabinet, the pedals in front of them, the order it all runs in, and
    where every knob is set. Line 6 saves one as an .hlx file. This gem opens
    that file and hands the rig back as ordinary Ruby - the blocks in order,
    what each one is, and every setting - so you can read a preset, compare two,
    or index a library of them. No hardware needed. The file is parsed by the
    same Rust code that talks to the pedal itself, so it reads here exactly as
    it does there.
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
