FROM node:24-bookworm-slim AS builder
WORKDIR /app
RUN corepack enable && corepack prepare pnpm@11.19.0 --activate
COPY apps/portal/package.json apps/portal/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY apps/portal ./
RUN pnpm build
FROM node:24-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production HOSTNAME=0.0.0.0
COPY --from=builder --chown=node:node /app/.next/standalone ./
COPY --from=builder --chown=node:node /app/.next/static ./.next/static
USER node
CMD ["node","server.js"]
