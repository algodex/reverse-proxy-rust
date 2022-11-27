const http = require('http');

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

const requestListener = async function (req, res) {
  await sleep(1000);  
  res.writeHead(200);
  const d = new Date();
  const starttext = 'This is som varios text here. This is som varios text here. This is som varios text here. This is som varios text here.';
  let text = '';
  for (let i = 0; i < 100; i++) {
    text += starttext;
  }
  res.end(text + d.toLocaleString());
  // res.end('Hello, World! Current Time: ' + d.toLocaleString());
}

const server = http.createServer(requestListener);
const port = 6000;
console.log('listening on ' + port);
server.listen(port);

