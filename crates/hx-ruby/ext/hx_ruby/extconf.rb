# frozen_string_literal: true

require "mkmf"
require "rb_sys/mkmf"

# Builds the Rust cdylib beside this file and installs it as
# lib/hx_ruby/hx_ruby.so, which lib/hx_ruby.rb then requires.
create_rust_makefile("hx_ruby/hx_ruby")
