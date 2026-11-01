/** @type {import('tailwindcss').Config} */
// Scans the maud markup (class names live in Rust string literals) so only
// the utilities actually used end up in the generated bundle. The output
// (assets/app.css) is committed and embedded in the binary via include_str!
// -- no CDN, no runtime build, works offline on the field tablet.
module.exports = {
  content: ["./src/**/*.rs"],
  theme: {
    extend: {
      fontFamily: {
        sans: ["ui-sans-serif", "system-ui", "Inter", "Segoe UI", "sans-serif"],
      },
    },
  },
  plugins: [],
};
