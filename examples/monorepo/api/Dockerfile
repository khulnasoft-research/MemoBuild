FROM node:18-alpine
WORKDIR /app
COPY package.json .
# RUN npm install
COPY api.js .
EXPOSE 4000
CMD ["npm", "start"]
