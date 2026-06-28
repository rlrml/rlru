Rocket Sense logo source: `rocket-sense-logo.svg`, copied from the Rocket Sense
web app's `web/public/brand/logo.svg`.

The app icon PNGs are exported directly from that SVG with `rsvg-convert` at
the sizes required by Dioxus, desktop entries, tray pixmaps, and Android
launcher assets. The generated PNGs keep the icon canvas transparent; they are
not cut out from a rasterized white background.
