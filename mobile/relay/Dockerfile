FROM node:22-alpine
WORKDIR /opt/pebrel/mobile/relay
COPY relay/package.json relay/package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts
COPY relay/*.mjs ./
COPY protocol/ ../protocol/
ENV BIND=0.0.0.0 PORT=8787 PEBREL_RELAY_CONFIG=/config/relay.config.json
USER node
CMD ["node", "server.mjs"]
