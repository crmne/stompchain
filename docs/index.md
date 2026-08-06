---
layout: home
title: stompchain
description: The open-source editor for Line 6 Helix and HX pedals. Fast, scriptable, and on every OS.
permalink: /
hero:
  name: stompchain
  text: Your Helix gear, on every OS
  tagline: The open-source editor for Line 6 Helix and HX pedals. A tiny download that opens fast, connects fast, and runs on Linux, macOS, and Windows.
  actions:
    - theme: brand
      text: Download
      link: /download/
    - theme: alt
      text: What is stompchain?
      link: /what-is-stompchain/
    - theme: alt
      text: GitHub
      link: https://github.com/crmne/stompchain
  image:
    src: /screenshot.png
    alt: "stompchain editing a preset on an HX Stomp: a drive, amp and cab on the main line with a delay and reverb on a parallel branch"
    width: 2000
    height: 1306

features:
  - icon: 🎛️
    title: Your whole rig at a glance
    details: Blocks, branches, and knobs laid out like the pedalboard they are. Drag a pedal below the line to run it in parallel, and move the fork wherever you want it.
  - icon: 🐧
    title: Every OS, including Linux
    details: HX Edit covers macOS and Windows. stompchain covers those and the Linux machine already sitting next to your pedalboard.
  - icon: ⚡
    title: Small and quick
    details: A few megabytes, opens in a blink, connects in a moment. No launcher, no installer ceremony, no waiting.
  - icon: 🔓
    title: Improving in the open
    details: HX Edit has not seen an update in a long while. stompchain is open source and moving, and the USB protocol behind it is documented for anyone to build on.
    link: https://github.com/crmne/stompchain/blob/main/PROTOCOL.md
    link_text: Read the protocol
  - icon: ⌨️
    title: Script everything
    details: A full command-line tool for backups, preset changes, and parameter tweaks. Automate your rig like the computer it secretly is.
  - icon: 🛡️
    title: Safe with your presets
    details: Presets travel as the device's own data, byte for byte, and every operation was verified against real hardware. What you save is exactly what was there.
---

<style>
  /* The hero image slot is sized for a square logo; the screenshot needs the
     room. Page-scoped overrides, so the theme stays untouched. */
  .VPHero .image-container {
    width: 100% !important;
    height: auto !important;
    transform: none !important;
  }
  .VPHero .image-src {
    position: relative !important;
    top: auto !important;
    left: auto !important;
    transform: none !important;
    width: 100% !important;
    height: auto !important;
    max-width: 100% !important;
    max-height: none !important;
    padding: 0 !important;
    border-radius: 12px;
    box-shadow: 0 12px 48px rgba(0, 0, 0, 0.45);
  }
  @media (max-width: 959px) {
    .VPHero .image {
      margin: 0 0 24px !important;
    }
  }
</style>

<div style="max-width: 960px; margin: 3rem auto 0; text-align: center;">
  <h2 style="font-size: 1.5rem; font-weight: 600; margin-bottom: 0.75rem;">Next up: a home for tones</h2>
  <p style="color: var(--vp-c-text-2); max-width: 640px; margin: 0 auto;">
    A community tone library is coming to stompchain: browse tones other
    players share, load one onto your pedal in a click, and publish your own
    straight from the editor. Star the
    <a href="https://github.com/crmne/stompchain">GitHub repository</a> to
    follow along.
  </p>
</div>
