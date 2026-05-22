const http = require('http');

const requestListener = function (req, res) {
    res.writeHead(200);
    res.end('{"status": "ok", "service": "api"}');
}

const server = http.createServer(requestListener);
server.listen(4000, () => {
    console.log("API server listening on port 4000!");
});
