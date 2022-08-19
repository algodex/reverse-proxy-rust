var PouchDB = require('pouchdb');

const db = new PouchDB('http://admin:dex@127.0.0.1:8000/escrow', { skip_setup: true });

db.info()
  .then(() => {
    console.log('db exists');
  })
  .catch(e => {
    console.log('db doesnt exist');
  });

