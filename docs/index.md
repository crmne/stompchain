---
layout: home
title: stompchain
description: The open-source editor for Line 6 HX pedals. Cross-platform GUI, scriptable CLI, and the protocol documentation behind both.
permalink: /
hero:
  name: stompchain
  text: Your HX Stomp, on every desktop
  tagline: An open-source editor for Line 6 HX pedals. Edit the signal chain, drag blocks into parallel branches, and script everything from the command line. Linux, macOS, and Windows.
  actions:
    - theme: brand
      text: Get stompchain
      link: /getting-started/
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
  - icon: 🎸
    title: The chain, drawn as it is wired
    details: The main line never bends. A parallel branch hangs below it between a draggable fork and merge. Drag a block down to run it in parallel, and choose how the split divides.
  - icon: 🐧
    title: Every desktop, including Linux
    details: HX Edit runs on macOS and Windows. stompchain runs there too, and on the Linux machine already sitting next to your pedalboard.
  - icon: 🔍
    title: Nothing lost in translation
    details: Presets travel as the device's own document, byte for byte. Copy, paste, import, export, and setlist backups carry everything, including what the editor does not model.
  - icon: ⌨️
    title: A CLI for everything
    details: Select presets, set parameters by name, back up setlists, upload impulse responses, and watch front-panel activity from scripts.
  - icon: 📖
    title: An open protocol
    details: The whole wire protocol is reverse-engineered and documented, with the reasoning and the dead ends. This editor is not the only thing that can ever be built on it.
    link: https://github.com/crmne/stompchain/blob/main/PROTOCOL.md
    link_text: Read PROTOCOL.md
  - icon: 🔬
    title: Verified against the hardware
    details: Every operation was checked against a real HX Stomp, down to byte-exact re-encoding of captured presets on every test run.
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
