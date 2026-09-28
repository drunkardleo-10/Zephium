// Usage: node crates/zephium-webext-windows/summarize-resources.mjs <results-directory>
// Process-family totals include the lab. Working-set sums count shared pages more than once.
import fs from 'node:fs';
import path from 'node:path';

const directory = process.argv[2];
if (!directory) throw new Error('Provide a probe results directory');
const median = values => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const mib = bytes => Math.round(bytes / 1048576 * 10) / 10;
for (const name of fs.readdirSync(directory).filter(name => name.endsWith('.resources.csv'))) {
  const lines = fs.readFileSync(path.join(directory, name), 'utf8').trim().split(/\r?\n/);
  const rows = lines.slice(1).map(line => {
    const [time, pid, processName, privateBytes, workingSetBytes, cpuSeconds] = JSON.parse(`[${line}]`);
    return {time: Date.parse(time), pid, processName, privateBytes: +privateBytes, workingSetBytes: +workingSetBytes, cpuSeconds: +cpuSeconds};
  });
  // Older captures timestamped each row separately. Group adjacent rows within
  // 500 ms; the runner samples approximately every two seconds.
  const samples = [];
  for (const row of rows) {
    let sample = samples.at(-1);
    if (!sample || row.time - sample.lastTime > 500) {
      sample = {time: row.time, lastTime: row.time, rows: []};
      samples.push(sample);
    }
    sample.lastTime = row.time;
    sample.rows.push(row);
  }
  const start = samples[0].time;
  for (const sample of samples) {
    sample.seconds = (sample.time - start) / 1000;
    sample.privateBytes = sample.rows.reduce((sum, row) => sum + row.privateBytes, 0);
    sample.workingSetBytes = sample.rows.reduce((sum, row) => sum + row.workingSetBytes, 0);
  }
  const duration = samples.at(-1).seconds;
  const stats = (from, to) => {
    const selected = samples.filter(sample => sample.seconds >= from && sample.seconds <= to);
    if (selected.length < 2) return null;
    let cpu = 0;
    const previous = new Map();
    for (const sample of selected) for (const row of sample.rows) {
      if (previous.has(row.pid)) cpu += Math.max(0, row.cpuSeconds - previous.get(row.pid));
      previous.set(row.pid, row.cpuSeconds);
    }
    const elapsed = selected.at(-1).seconds - selected[0].seconds;
    return {
      fromSeconds: selected[0].seconds, toSeconds: selected.at(-1).seconds,
      samples: selected.length,
      privateMiBMedian: mib(median(selected.map(sample => sample.privateBytes))),
      privateMiBMin: mib(Math.min(...selected.map(sample => sample.privateBytes))),
      privateMiBMax: mib(Math.max(...selected.map(sample => sample.privateBytes))),
      workingSetMiBMedian: mib(median(selected.map(sample => sample.workingSetBytes))),
      processesMin: Math.min(...selected.map(sample => sample.rows.length)),
      processesMax: Math.max(...selected.map(sample => sample.rows.length)),
      cpuSeconds: Math.round(cpu * 100) / 100,
      cpuPercentOneCore: Math.round(cpu / elapsed * 10000) / 100
    };
  };
  console.log(JSON.stringify({name, durationSeconds: duration, samples: samples.length,
    late: stats(Math.max(0, duration - 30), duration),
    ...(duration > 180 ? {minuteTwo: stats(60, 120), finalMinute: stats(duration - 60, duration), afterWarmup: stats(60, duration)} : {})
  }, null, 2));
}
