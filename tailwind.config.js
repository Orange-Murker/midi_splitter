/** @type {import('tailwindcss').Config} */
module.exports = {
	content: {
		files: ["*.html", "./src/**/*.rs"],
	},
	theme: {
		fontFamily: {
			sans: ["Graphik", "sans-serif"],
			serif: ["Merriweather", "serif"],
		},
		extend: {},
	},
	plugins: [],
};
