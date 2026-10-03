// The browser only ever talks to this origin; Next proxies /api/* to the Rust API.
// That sidesteps CORS (the backend sends no CORS headers) without touching it.
const API_URL = process.env.API_URL ?? "http://localhost:8080";

/** @type {import('next').NextConfig} */
const nextConfig = {
  async rewrites() {
    return [{ source: "/api/:path*", destination: `${API_URL}/:path*` }];
  },
};

export default nextConfig;
