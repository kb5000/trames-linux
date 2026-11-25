// SPDX-License-Identifier: GPL-3.0-or-later
const {Worker, isMainThread, parentPort, workerData} = require('worker_threads');
function calculate(count, seed) {
  let x = seed;
  for (let i = 0; i < count; i++) x = Math.imul(x ^ (x >>> 13), 1664525) + 1013904223 | 0;
  return x >>> 0;
}
if (!isMainThread) {
  calculate(100000, workerData.index + 1);
  parentPort.postMessage({ready: true});
  Atomics.wait(new Int32Array(workerData.start), 0, 0);
  parentPort.postMessage({checksum: calculate(workerData.iterations, workerData.index + 1)});
} else {
  const concurrency = Number(process.argv[2]), iterations = Number(process.argv[3]);
  if (!(concurrency > 0 && concurrency <= 16 && iterations > 0)) throw Error('invalid work');
  const expected = Array.from({length: concurrency}, (_,index) => calculate(iterations, index + 1));
  const start = new SharedArrayBuffer(4), workers = [];
  let ready = 0, finished = 0, began, checksum = 0;
  for (let index = 0; index < concurrency; index++) {
    const worker = new Worker(__filename, {workerData: {index, iterations, start}});
    workers.push(worker);
    worker.on('error', error => {throw error});
    worker.on('message', message => {
      if (message.ready) {
        if (++ready === concurrency) {
          began = process.hrtime.bigint();
          Atomics.store(new Int32Array(start), 0, 1); Atomics.notify(new Int32Array(start), 0);
        }
      } else {
        if (message.checksum !== expected[index]) throw Error('wrong worker result');
        checksum += message.checksum;
        if (++finished === concurrency) {
          console.log(JSON.stringify({mode: 'node', concurrency, tasks: concurrency, iterations, checksum,
            work_ns: Number(process.hrtime.bigint() - began)}));
        }
      }
    });
  }
}
