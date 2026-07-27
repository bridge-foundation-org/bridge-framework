/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    './app/**/*.{js,ts,jsx,tsx,mdx}',
    './components/**/*.{js,ts,jsx,tsx,mdx}',
  ],
  theme: {
    extend: {
      colors: {
        bridge: {
          50: '#f8f9ff',
          100: '#f0f1ff',
          500: '#6366f1',
          600: '#4f46e5',
          700: '#4338ca',
          900: '#1e1b4b',
        },
      },
      typography: (theme) => ({
        DEFAULT: {
          css: {
            color: theme('colors.gray.700'),
            a: {
              color: theme('colors.bridge.600'),
              '&:hover': {
                color: theme('colors.bridge.700'),
              },
            },
            'code::before': {
              content: '""',
            },
            'code::after': {
              content: '""',
            },
            code: {
              backgroundColor: theme('colors.gray.100'),
              padding: theme('spacing.1'),
              borderRadius: theme('borderRadius.md'),
            },
            pre: {
              backgroundColor: theme('colors.gray.900'),
            },
          },
        },
      }),
    },
  },
  plugins: [require('@tailwindcss/typography')],
}
