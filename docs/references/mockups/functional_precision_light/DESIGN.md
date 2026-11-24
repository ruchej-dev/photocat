---
name: Functional Precision Light
colors:
  surface: '#fff8f5'
  surface-dim: '#e9d7cb'
  surface-bright: '#fff8f5'
  surface-container-lowest: '#ffffff'
  surface-container-low: '#fff1e9'
  surface-container: '#fdeade'
  surface-container-high: '#f7e5d9'
  surface-container-highest: '#f2dfd3'
  on-surface: '#231a13'
  on-surface-variant: '#554336'
  inverse-surface: '#392e26'
  inverse-on-surface: '#ffede3'
  outline: '#887364'
  outline-variant: '#dbc2b0'
  surface-tint: '#904d00'
  primary: '#8d4b00'
  on-primary: '#ffffff'
  primary-container: '#b15f00'
  on-primary-container: '#fffbff'
  inverse-primary: '#ffb77d'
  secondary: '#515f74'
  on-secondary: '#ffffff'
  secondary-container: '#d5e3fc'
  on-secondary-container: '#57657a'
  tertiary: '#0051d5'
  on-tertiary: '#ffffff'
  tertiary-container: '#316bf3'
  on-tertiary-container: '#fefcff'
  error: '#ba1a1a'
  on-error: '#ffffff'
  error-container: '#ffdad6'
  on-error-container: '#93000a'
  primary-fixed: '#ffdcc3'
  primary-fixed-dim: '#ffb77d'
  on-primary-fixed: '#2f1500'
  on-primary-fixed-variant: '#6e3900'
  secondary-fixed: '#d5e3fc'
  secondary-fixed-dim: '#b9c7df'
  on-secondary-fixed: '#0d1c2e'
  on-secondary-fixed-variant: '#3a485b'
  tertiary-fixed: '#dbe1ff'
  tertiary-fixed-dim: '#b4c5ff'
  on-tertiary-fixed: '#00174b'
  on-tertiary-fixed-variant: '#003ea8'
  background: '#fff8f5'
  on-background: '#231a13'
  surface-variant: '#f2dfd3'
  canvas-base: '#f8fafc'
  surface-card: '#ffffff'
  surface-subtle: '#f1f5f9'
  surface-recessed: '#e2e8f0'
  border-subtle: '#e2e8f0'
  border-medium: '#cbd5e1'
  border-strong: '#94a3b8'
  text-primary: '#0f172a'
  text-secondary: '#475569'
  text-muted: '#94a3b8'
  amber-focus: '#b45309'
  amber-subtle: '#fef3c7'
  amber-soft: '#fde68a'
  status-success: '#16a34a'
  status-info: '#2563eb'
  status-danger: '#dc2626'
  danger-subtle: '#fee2e2'
typography:
  headline-lg:
    fontFamily: Inter
    fontSize: 18px
    fontWeight: '600'
    lineHeight: 24px
    letterSpacing: -0.01em
  headline-md:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: '600'
    lineHeight: 20px
    letterSpacing: -0.005em
  headline-sm:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: '600'
    lineHeight: 16px
    letterSpacing: 0.01em
  body-md:
    fontFamily: Inter
    fontSize: 13px
    fontWeight: '400'
    lineHeight: 18px
  body-sm:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 16px
  mono-data:
    fontFamily: JetBrains Mono
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 16px
  mono-numeric:
    fontFamily: JetBrains Mono
    fontSize: 11px
    fontWeight: '500'
    lineHeight: 14px
  label-caps:
    fontFamily: Inter
    fontSize: 10px
    fontWeight: '600'
    lineHeight: 12px
    letterSpacing: 0.06em
  label-compact:
    fontFamily: JetBrains Mono
    fontSize: 10px
    fontWeight: '400'
    lineHeight: 12px
spacing:
  gutter: 0.5rem
  gutter-compact: 0.25rem
  margin: 0.5rem
  margin-dense: 0.25rem
  space-xs: 0.125rem
  space-sm: 0.25rem
  space-md: 0.5rem
  space-lg: 0.75rem
  space-xl: 1rem
---

## Brand & Style

This design system delivers a high-density, utility-first desktop environment engineered for photo cataloging, deduplication, and raw asset curation in a focused light mode. Conceived in the tradition of technical inspection workstations, optical laboratory consoles, and professional studio grading software, it rejects playful roundness, decorative gradients, and frivolous negative space.

### Key Pillars
- **Technical Rigor & Neutrality:** The chrome recedes behind pure neutral light surfaces (`#F8FAFC`, `#FFFFFF`), allowing photographic previews to be judged without color contamination or artificial dark-adaptation bias.
- **Instrument-Grade Density:** Tight spatial packing, compact row heights, and high-legibility typographic scales maximize the field of view for high-volume catalogs and complex metadata tables.
- **Amber Signal Clarity:** A focused amber spectrum (`#D97706`, `#B45309`) anchors confirmed actions (`оставить`, active selections, sorting commitments) with strict WCAG AA contrast against light surfaces.
- **Unambiguous Pragmatism:** The aesthetic favors planar stratification, razor-sharp 1px structural framing, and square physical affordances over skeuomorphic fluff or ambient blurs.

## Colors

The light mode palette uses an architectural range of cool, slate-tinted whites and grays that maintain visual comfort during extended curation sessions while preventing chromatic cast on previewed imagery.

### Surface System
- **Canvas Base (`#F8FAFC`):** The default window background, dock recesses, and empty canvas spaces.
- **Surface Level 1 / Card (`#FFFFFF`):** High-priority inspection containers, photo card backing, modal dialogs, and content panels.
- **Surface Level 2 / Subtle (`#F1F5F9`):** Sidebar panels, toolbar strips, table header rows, and unselected tab wells.
- **Surface Level 3 / Recessed (`#E2E8F0`):** Inset input fields, inactive control wells, splitter bars, and progress tracks.

### Delineation & Dividers
- **Subtle Border (`#E2E8F0`):** Standard panel separation, cell divides, and card perimeters.
- **Medium Border (`#CBD5E1`):** Active structural boundaries, window partitions, and nested data tables.
- **Strong Border (`#94A3B8`):** Drag handles, splitter dividers, and inactive checkbox outlines.

### Signal & Accent Mechanics
- **Primary Amber (`#D97706`):** The primary signal color. Used for active selection rings, confirmation buttons (`оставить`), active filter chips, and primary progress indicators. Provides a contrast ratio > 4.5:1 against `#FFFFFF` and `#F8FAFC`.
- **Amber Deep / Hover (`#B45309`):** Hover and pressed states for primary amber actions; text labels on light amber badges.
- **Amber Subtle (`#FEF3C7`):** Background fill for selected rows, active marquee bounds, and highlighted tag badges.
- **Status Green (`#16A34A`):** Verified files, completed batch tasks, and clean storage indicators.
- **Status Blue (`#2563EB`):** Metadata sync, indexing in progress, and informational notices.
- **Status Red (`#DC2626`):** Rejection (`пометить лишним`), permanent file deletion, and operational errors. Paired with `#FEE2E2` for alert backgrounds.

### Content Contrast
- **High-Emphasis Text (`#0F172A`):** Primary file titles, panel headers, key numerical indicators.
- **Medium-Emphasis Text (`#475569`):** EXIF parameter names, secondary labels, active breadcrumbs, folder paths.
- **Muted Text (`#94A3B8`):** Disabled fields, placeholder strings, keyboard shortcut hints.

## Typography

The typographic hierarchy is optimized for dense visual scanning, structural organization, and subpixel metric alignment on high-resolution displays.

### Font Roles
- **Primary Shell (`Inter`):** Renders all contextual UI labels, tree hierarchies, action buttons, dialogues, and metadata headers. Its optical consistency provides clear legibility at compact sizes (10px–14px).
- **Technical Monospace (`JetBrains Mono`):** Renders raw numerical indicators, EXIF fields (aperture, shutter speed, ISO, focal length), byte values, timestamps, file extensions, and checksum hashes. Tabular lining figures are enforced across all counters.

### Typographic Principles
- **Restrained Scaling:** Maximum headline scale is bounded at 18px (`headline-lg`), ensuring UI headers never dominate screen space intended for image previews.
- **Uppercase Framing:** Category dividers and sidebar sections utilize `label-caps` in uppercase styling with positive letter tracking (`0.06em`) to establish visual separation without introducing vertical bulk.
- **Monospaced Counters:** Any dynamic tally (e.g., `4,210 / 12,800 фото`) must use `mono-numeric` to prevent jitter during batch indexing or viewport recalculations.

## Layout & Spacing

The layout model is a docked, multi-pane desktop workbench arranged around a primary central asset viewport. All dimensions step along a deterministic 2px/4px/8px rhythm.

### Layout Architecture
- **Navigation Dock (Left):** Width spans 240px–300px. Houses file trees, smart queries, catalog tags, and archive bucket metrics.
- **Central Canvas (Center):** Fluid viewport containing virtualized photo grids or multi-pane comparative loupes. Grid items are separated by `gutter-compact` (4px) or `gutter` (8px).
- **Inspector Deck (Right):** Width spans 280px–360px. Encapsulates EXIF readouts, histogram visualization, color profile tags, and duplication clusters.
- **Global Strip (Top, 36px):** Global breadcrumb path, search query inputs, archive storage gauges, and view mode toggle buttons.
- **Status Strip (Bottom, 26px):** Pinned system bar featuring active write queues, batch progress percentages, and emergency interrupt triggers (`Стоп`).

### Splitters and Resizing
- Splitter seams between docked panels are strictly 1px wide using `#CBD5E1`.
- The interactive hit zone extends 3px on either side of the visual seam to ensure seamless dragging without visual clutter.

## Elevation & Depth

Visual hierarchy is conveyed entirely through planar tonal stepping and crisp 1px structural framing. Drop shadows, directional lights, and blur filters are excluded to maintain clinical rendering precision and eliminate GPU draw overhead.

### Elevation Hierarchy
- **Level 0 (Canvas Base, `#F8FAFC`):** Deepest structural level; application window background, virtualized grid recess, scroll viewport foundations.
- **Level 1 (Docked Surfaces, `#F1F5F9`):** Stationary side panels, tool strips, and unselected tab headers.
- **Level 2 (Containers & Cards, `#FFFFFF`):** High-priority inspection panels, photo thumbnail cells, active tab cards, and flyout palettes.
- **Level 3 (Interactive Insets, `#E2E8F0`):** Inset search fields, checkbox backgrounds, and progress gauge tracks.

### Crisp Borders
- Every panel boundary, toolbar edge, and table divider is defined by a 1px solid border (`#E2E8F0` for inner components, `#CBD5E1` for major window sections).
- Menus and context popovers sit on Surface Level 2 (`#FFFFFF`) with a 1px perimeter border in `#94A3B8`.
- Focus states and selected elements override standard borders with a 1px or 2px solid line in primary amber (`#D97706`), with zero drop shadow or outer ring spread.

## Shapes

The design system uses a strict 0px border radius across all controls, containers, badges, and image framing.

### Structural Rationale
- **Screen Real Estate:** Sharp, unrounded boundaries maximize active pixel area for image thumbnails and high-density metadata tables.
- **Subpixel Fidelity:** Straight edges snap cleanly to integer screen coordinates, eliminating antialiasing artifacts on low- and high-DPI displays alike.
- **Tool Metaphor:** Reinforces the application's nature as an engineering utility and optical photo-sorting workstation.

### Geometric Rules
- Buttons, input fields, badges, and dialogs have 0px corner radii.
- Image viewports and thumbnail frames maintain sharp 90-degree corners with no clipping masks.
- Selection indicators, active states, and focus frames are rectangular inset lines.

## Components

### Buttons & Operational Actions
- **Primary / Confirmation (`оставить`, `выгрузить копии`):** Surface `#D97706`, text `#FFFFFF`, font weight 600, padding 4px 12px, 0px border radius. Hover state shifts surface to `#B45309`. Focus creates an inset 1px solid border in `#FFFFFF`.
- **Neutral Command:** Surface `#FFFFFF`, border 1px solid `#CBD5E1`, text `#0F172A`, padding 4px 10px. Hover shifts surface to `#F1F5F9` with border `#94A3B8`.
- **Destructive Command (`пометить лишним`, `в корзину`):** Surface transparent, border 1px solid `#DC2626`, text `#DC2626`, padding 4px 10px. Hover transitions surface to `#FEE2E2`.
- **Operational Interrupt (`Стоп`):** Height 20px, surface `#E2E8F0`, border 1px solid `#CBD5E1`, text `#DC2626`, font family `JetBrains Mono` at 11px uppercase. Hover shifts to `#FEE2E2`.

### Thumbnail Cards & Grid Viewport
- **Thumbnail Frame:** Surface `#FFFFFF`, border 1px solid `#E2E8F0`, padding 0px.
- **Metadata Footer:** Positioned directly beneath preview image; height 22px, surface `#F1F5F9`, border-top 1px solid `#E2E8F0`, text `#475569`, font `JetBrains Mono` 10px showing filename and dimensions.
- **Selected State (`оставить`):** 2px solid `#D97706` inset border surrounding the entire thumbnail card. A solid amber badge `[ОСТАВЛЕНО]` appears at the top-right corner with text `#FFFFFF` in `JetBrains Mono` 10px.
- **Rejected State (`пометить лишним`):** 30% translucent white overlay across the image frame, paired with a 1px solid `#DC2626` border and a red status flag `[ЛИШНЕЕ]`.

### Progress & Status Bars
- **Track:** Height 14px, surface `#E2E8F0`, border 1px solid `#CBD5E1`.
- **Fill:** Solid `#D97706` (primary task) or `#2563EB` (indexing metadata) without gradients or animations.
- **Text Readout:** Directly adjacent or overlaid; rendered in `JetBrains Mono` 11px: `34,120 / 68,400 (49.8%) — 142 MB/s`.

### Metadata Chips & Tag Badges
- **Default Chip:** Height 20px, surface `#F1F5F9`, border 1px solid `#CBD5E1`, text `#475569`, font `JetBrains Mono` 11px, horizontal padding 6px.
- **Active / Filtered Chip:** Surface `#FEF3C7`, border 1px solid `#D97706`, text `#B45309`, font weight 500.
- **Dismiss Control (`убрать тег`):** 12x12px square action `[×]` aligned to the right edge. Hover changes text color to `#DC2626`.

### Data Tables & Tree Navigation
- **Row Heights:** Standard density 22px, compact density 18px.
- **Row Hover:** Surface `#F1F5F9`.
- **Selected Row:** Surface `#FEF3C7`, text `#0F172A`, left border 2px solid `#D97706`.
- **Grid Lines:** 1px horizontal dividers using `#E2E8F0`. Indentation uses continuous 1px vertical guide lines in `#CBD5E1`.

### Inputs & Inspection Fields
- **Text & Filter Inputs:** Height 24px, surface `#FFFFFF`, border 1px solid `#CBD5E1`, font `Inter` or `JetBrains Mono` 12px, text `#0F172A`, horizontal padding 6px.
- **Input Focus:** Border switches to 1px solid `#D97706`. Outer glows or shadows are omitted.
- **Checkboxes & Segment Selectors:** 14x14px square bounding box. Checked state features a solid `#D97706` fill with an internal white square node.