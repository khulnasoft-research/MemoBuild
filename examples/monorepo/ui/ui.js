const http = require('http');

const requestListener = function (req, res) {
  res.writeHead(200);
  res.end('<h1>MemoBuild UI Dashboard</h1><p>Served from the UI microservice!</p>');
}

const server = http.createServer(requestListener);
server.listen(3000, () => {
    console.log("UI server listening on port 3000!");
});
