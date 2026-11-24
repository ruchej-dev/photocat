---
name: Functional Precision Desktop
colors:
  surface: '#111317'
  surface-dim: '#111317'
  surface-bright: '#37393e'
  surface-container-lowest: '#0c0e12'
  surface-container-low: '#1a1c20'
  surface-container: '#1e2024'
  surface-container-high: '#282a2e'
  surface-container-highest: '#333539'
  on-surface: '#e2e2e8'
  on-surface-variant: '#d8c3ad'
  inverse-surface: '#e2e2e8'
  inverse-on-surface: '#2f3035'
  outline: '#a08e7a'
  outline-variant: '#534434'
  surface-tint: '#ffb95f'
  primary: '#ffc174'
  on-primary: '#472a00'
  primary-container: '#f59e0b'
  on-primary-container: '#613b00'
  inverse-primary: '#855300'
  secondary: '#bdc7db'
  on-secondary: '#273140'
  secondary-container: '#404a5a'
  on-secondary-container: '#afb9cc'
  tertiary: '#8fd5ff'
  on-tertiary: '#00344a'
  tertiary-container: '#1abdff'
  on-tertiary-container: '#004966'
  error: '#ffb4ab'
  on-error: '#690005'
  error-container: '#93000a'
  on-error-container: '#ffdad6'
  primary-fixed: '#ffddb8'
  primary-fixed-dim: '#ffb95f'
  on-primary-fixed: '#2a1700'
  on-primary-fixed-variant: '#653e00'
  secondary-fixed: '#d9e3f7'
  secondary-fixed-dim: '#bdc7db'
  on-secondary-fixed: '#121c2a'
  on-secondary-fixed-variant: '#3d4757'
  tertiary-fixed: '#c5e7ff'
  tertiary-fixed-dim: '#7fd0ff'
  on-tertiary-fixed: '#001e2d'
  on-tertiary-fixed-variant: '#004c6a'
  background: '#111317'
  on-background: '#e2e2e8'
  surface-variant: '#333539'
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
    letterSpacing: 0.02em
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

This design system delivers a high-density, utility-first desktop environment engineered for managing massive 60–70 GB local family photo archives. Rooted in professional creative tools like Blender, Lightroom, and IDE-native interfaces, it eliminates decorative fluff, rounded consumer-app trends, and ambiguous iconography in favor of pure operational efficiency, visual endurance, and technical authority.

### Key Pillars
- **Strict Utilitarianism:** Information density takes precedence over ornamental whitespace. The interface operates as a responsive command station for cataloging, deduplicating, sorting, and batch-processing images.
- **Instrument-Grade Predictability:** Interactions prioritize instant state clarity. State changes (flags, tags, deletions) rely on hard edges, sharp toggles, and unambiguous monochromatic tiers.
- **Zero Distraction Palette:** The archive preview itself is the hero. The surrounding shell remains receded within deep, cool industrial grays, stepping forward only via a single amber signal accent denoting explicit user confirmation (`оставить`, active selections, progress confirmation).
- **Direct Native Tone:** Microcopy rejects colloquial or consumer metaphors. Actions use plain imperative engineering terminology: `пометить лишним`, `убрать тег`, `в корзину приложения`, `выгрузить копии`, `оставить`.

## Colors

The palette establishes an ergonomic, dark-native hierarchy engineered to mitigate eye strain over hours of triage. A single purposeful amber-orange (`#F59E0B`) cuts through neutral cool-slate layers to denote verified status, focus, and explicit user commitment.

### Palette Architecture
- **Canvas Base (`#121418`):** Deep charcoal ground for viewports, asset canvases, and root application backdrops.
- **Surface Level 1 (`#1A1D24`):** Side panels, inspect sidebars, tool headers, and dock bars.
- **Surface Level 2 (`#222630`):** Component backgrounds, list item active rows, grouped control trays, and card panels.
- **Surface Level 3 / Field Base (`#2C3240`):** Input fields, inactive segment buttons, table headers, and scroll tracks.
- **Structural Borders (`#374151`):** 1px delineations between panels, splitters, tree rows, and tabular data boundaries.
- **Primary Signal (`#F59E0B`):** Reserved exclusively for active selection states, "Keep" (`оставить`) commitments, focused input outlines, active batch indicators, and progress bars.
- **Signal Muted (`rgba(245, 158, 11, 0.15)`): Selection fills, hover badges for confirmed tags, and multi-selection marquee masks.
- **Critical Destruction (`#EF4444`):** Dedicated to destructive operations (`в корзину приложения`, `пометить лишним`). Never used as decoration.

### Content Contrast
- **Text High-Emphasis (`#F3F4F6`):** Primary file names, quantitative counters, active tab titles.
- **Text Medium-Emphasis (`#9CA3AF`):** EXIF parameters, folder paths, metadata keys, unselected labels.
- **Text Disabled/Dim (`#4B5563`):** Inactive indicators, empty-state placeholders, shortcut cues.

## Typography

Typography balances rapid scanning with high-fidelity metric presentation. Standard interfaces rely on `Inter` for clarity and optical neutrality, while `JetBrains Mono` handles all technical values, file extensions, ISO ratings, timestamps, dimensions, and indexing counters.

### Type Application Rules
- **Numerical Alignment:** All file sizes, aspect ratios, focal lengths, shutter speeds, and execution counters (`34,120 / 68,400`) must strictly render in `JetBrains Mono` with tabular lining figures enabled.
- **Compact Vertical Headers:** Panel categories and metadata groups use `label-caps` in uppercase format with wide tracking for clear spatial division at compact scales without excessive pixel padding.
- **Density Over Scale:** Heading sizes stay capped at 18px maximum, matching native desktop conventions. Font weights rarely drop below 400 to maintain crisp subpixel rendering against dark surfaces in egui/OpenGL viewports.

## Layout & Spacing

The layout model is a docked, multi-panel desktop grid optimized for multi-pane synchronization, resizing splitters, and persistent visual oversight. Spacing operates strictly on a deterministic 2px/4px/8px stepping grid.

### Layout Anatomy
- **Docked Panels:** Fixed-width sidebars (Left: File tree, folder index, query filters at 240px–320px; Right: EXIF data, tag inspector, duplication analysis at 280px–360px).
- **Central Canvas:** Scalable photo grid matrix featuring virtualized rendering. Grid cells adjust dynamically to thumbnail resolution settings with continuous 4px or 8px gutters.
- **Top Utility Strip:** 32px height bar housing global archive status, storage meter (`62.4 GB / 70.0 GB`), active filter tags, search queries, and batch mode triggers.
- **Bottom Process Deck:** 28px height status ribbon anchored to the window bottom, presenting ongoing pipeline status, write queue counts, exact progress meters, and operational interrupt controls (`Стоп`).

### Resizing and Splitter Rules
- Splitter handles measure exactly 1px visually, featuring a 5px invisible hit-area.
- Component gutters inside toolbars never exceed `0.5rem` (8px). Gaps between micro-controls (e.g., zoom step buttons, pagination arrows) stay pinned to `0.25rem` (4px).

## Elevation & Depth

This system intentionally eliminates drop shadows, blurs, and skeuomorphic illusions. Depth and z-axis indexing are established purely through tonal stepping and 1px crisp separation lines.

### Surface Tiers
- **Tier 0 (Root Base, `#121418`):** Main viewport, thumbnail workspace, background behind image viewing panes.
- **Tier 1 (Panels & Toolbars, `#1A1D24`):** Fixed dockable sidebars, utility strips, bottom status bars.
- **Tier 2 (Containers & Active Regions, `#222630`):** Inset inspection boxes, nested folder item rows, preview metadata trays.
- **Tier 3 (Interactive Controls, `#2C3240`):** Text entry fields, idle buttons, checkbox wells, progress tracks.

### Structural Lines
- Every panel boundary, column header divider, and control cluster separation is reinforced with a 1px solid `#374151` line.
- Popovers, context menus, and quick-filter modals sit on Surface Tier 2 (`#222630`) framed with a 1px border of `#4B5563` and zero shadow blur.
- Focus borders on input fields or active card selections override the border color directly with the primary amber `#F59E0B` without outer glow or spread.

## Shapes

The shape system adopts a zero-radius (`0px`), strictly sharp mechanical aesthetic. By dropping all corner rounding, controls maximize usable viewport territory, ensure flawless alignment with subpixel display grids, and evoke the precision of laboratory and professional editing consoles.

### Geometric Rules
- **Buttons, Inputs, Badges, Tabs:** 0px border radius across all states.
- **Image Thumbnails:** Sharp, unrounded rectangle viewports. Active selection states append a 2px inset border of `#F59E0B` directly over the thumbnail edge.
- **Checkboxes & Segment Toggles:** Pure rectangular bounding boxes. Radio indicators use square concentric nodes rather than circles.
- **Separators & Splitters:** Crisp rectangular lines of 1px thickness.

## Components

### Buttons & Operational Actions
- **Primary / Confirmation (`оставить`, `выгрузить копии`):** Surface `#F59E0B`, text `#121418`, font weight 600, padding 4px 12px, 0px radius. Hover shifts to `#D97706`. Focus displays a 1px white internal inset border.
- **Neutral Command:** Surface `#222630`, border 1px solid `#374151`, text `#F3F4F6`, padding 4px 10px. Hover shifts surface to `#2C3240`.
- **Destructive Command (`пометить лишним`, `в корзину приложения`):** Surface transparent, border 1px solid `#EF4444`, text `#EF4444`. Hover fills surface with `rgba(239, 68, 68, 0.15)`.
- **Interrupt Button (`Стоп`):** High-visibility utility button placed directly next to active operations. Height 20px, surface `#374151`, text `#F3F4F6`, border 1px solid `#4B5563`, font family `JetBrains Mono` at 11px uppercase.

### Progress & Processing Bars
- **Track:** Height 14px to 16px, background `#1A1D24`, border 1px solid `#374151`.
- **Fill:** Solid `#F59E0B` fill with no gradients or striping.
- **Embedded Metrics:** Centered overlay or inline right suffix displaying precise ratios and percentages in `JetBrains Mono` 11px: `34,120 / 68,400 (49.8%) — 128 MB/s`.

### Metadata Chips & Tagging
- **Tag Element:** Height 20px, surface `#1A1D24`, border 1px solid `#374151`, typography `JetBrains Mono` 11px, text `#9CA3AF`.
- **Selected Tag:** Border 1px solid `#F59E0B`, text `#F59E0B`, surface `rgba(245, 158, 11, 0.08)`.
- **Tag Removal Trigger (`убрать тег`):** Inline square button `[×]` measuring 12x12px at the trailing edge. Hover highlights text to `#EF4444`.

### Lists & Tree Navigators
- **Row Architecture:** 22px height per row in standard mode; 18px in compact mode. Alternating subtle striping is omitted; depth is established through indentation markers (12px guide lines).
- **Active Selection:** Full-width background `#2C3240` with an uncompromising 2px solid `#F59E0B` left edge indicator. Text color flips to `#F3F4F6`.
- **Hover State:** Background `#1F242D` without left edge indicator.

### Input Fields & Filter Bars
- **Text Inputs:** Height 24px, surface `#121418`, border 1px solid `#374151`, font `Inter` or `JetBrains Mono` 12px, text `#F3F4F6`. Padding 2px 6px.
- **Active Focus:** Border transitions instantly to 1px solid `#F59E0B`. No drop shadow.

### Thumbnail Card Cells
- **Card Framing:** 1px border `#222630`. Padding 0px. Bottom label band sits directly beneath the preview (height 20px, surface `#1A1D24`, font `JetBrains Mono` 10px showing filename and resolution).
- **Selection Overlay:** When marked with `оставить`, top-right corner displays a solid amber flag `[ВЫБРАНО]` or check glyph framed in `#121418`. If marked via `пометить лишним`, image displays a 40% darkened mask with a red diagonal strike or corner badge.