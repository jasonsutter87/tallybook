/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    "./layouts/**/*.html",
    "./content/**/*.{html,md}",
    "./data/**/*.yaml",
    "./static/js/**/*.js",
    "./assets/scss/**/*.scss",
  ],
  // Minimal safelist - only for DevTools testing and CMS content
  // Most classes are auto-detected from templates by Tailwind JIT
  safelist: [
    { pattern: /^(w|h)-(full|auto|1\/2|1\/3|2\/3|1\/4|3\/4)$/ },
    { pattern: /^(p|m|py|px|my|mx|pt|pb|mt|mb)-[0-8]$/ },
    { pattern: /^gap-[0-6]$/ },
    { pattern: /^text-(xs|sm|base|lg|xl|2xl|3xl)$/ },
    { pattern: /^font-(normal|medium|semibold|bold)$/ },
    { pattern: /^(hidden|block|flex|grid)$/ },
    { pattern: /^text-(left|center|right)$/ },
    'focus:outline-none', 'focus:ring-2', 'focus:ring-white', 'focus:ring-offset-2',
  ],
  theme: {
    extend: {
      // Tallybook's palette (scss/shared/_variables.scss), plus the Bootstrap names
      colors: {
        'ink': '#0b1b33',
        'brand': '#0aa2c0',
        'primary': '#087f99',
        'secondary': '#4a5a75',
        'success': '#0e9f6e',
        'danger': '#e0474c',
        'warning': '#c77700',
        'info': '#0aa2c0',
        'light': '#f3f5f9',
        'dark': '#0b1b33',
      },
      fontFamily: {
        'sans': ['"Plus Jakarta Sans"', 'ui-sans-serif', 'system-ui', 'sans-serif'],
      },
      maxWidth: {
        '1340': '1340px',
      },
    },
    // Bootstrap breakpoints
    screens: {
      'sm': '576px',
      'md': '768px',
      'lg': '992px',
      'xl': '1200px',
      'xxl': '1400px',
    },
  },
  plugins: [],
}
