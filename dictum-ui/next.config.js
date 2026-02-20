const path = require("path");

/** @type {import('next').NextConfig} */
const nextConfig = {
  // Output static files for Tauri to serve
  output: "export",

  // Disable image optimization (not available in static export)
  images: {
    unoptimized: true,
  },

  // Tauri expects the build output at ../dictum-ui/out
  distDir: "out",

  // Resolve @shared/* alias to ../shared/ (outside project root)
  webpack: (config) => {
    config.resolve.alias = {
      ...config.resolve.alias,
      "@shared": path.resolve(__dirname, "../shared"),
    };
    return config;
  },
};

module.exports = nextConfig;
