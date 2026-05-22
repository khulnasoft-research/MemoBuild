FROM node:18-alpine
WORKDIR /app
COPY package.json .
# RUN npm install
COPY ui.js .
EXPOSE 3000
CMD ["npm", "start"]
