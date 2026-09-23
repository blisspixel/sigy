import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.dirname(fileURLToPath(import.meta.url));
function parseCompact(buffer) {
  let offset = 0;
  function byte() { if (offset >= buffer.length) throw Error('Truncated footer'); return buffer[offset++]; }
  function unsigned() {
    let n = 0n;
    for (let i=0; i<10; i++) { const b=byte(); n |= BigInt(b & 127) << BigInt(i*7); if (!(b & 128)) return n; }
    throw Error('Oversized varint');
  }
  function integer() { const n=unsigned(); const v=(n>>1n)^(-(n&1n)); if(v>BigInt(Number.MAX_SAFE_INTEGER)||v<BigInt(Number.MIN_SAFE_INTEGER))throw Error('Unsafe integer'); return Number(v); }
  function value(type, depth=0) {
    if(depth>32)throw Error('Excessive nesting');
    if(type===1)return true;
    if(type===2)return false;
    if(type===3){const b=byte();return b>127?b-256:b;}
    if(type>=4&&type<=6)return integer();
    if(type===7){if(offset+8>buffer.length)throw Error('Truncated double');const d=buffer.readDoubleLE(offset);offset+=8;return d;}
    if(type===8){const n=Number(unsigned());if(n<0||offset+n>buffer.length)throw Error('Invalid binary length');const b=buffer.subarray(offset,offset+n);offset+=n;return b;}
    if(type===9||type===10){const h=byte();let n=h>>4;if(n===15)n=Number(unsigned());if(n>10000)throw Error('Large list');const a=[];for(let i=0;i<n;i++)a.push(value(h&15,depth+1));return a;}
    if(type===11){const n=Number(unsigned());if(n>10000)throw Error('Large map');if(!n)return [];const h=byte();const a=[];for(let i=0;i<n;i++)a.push([value(h>>4,depth+1),value(h&15,depth+1)]);return a;}
    if(type===12){const result={};let last=0;while(true){const h=byte();if(h===0)break;const d=h>>4;const id=d?last+d:integer();result[id]=value(h&15,depth+1);last=id;}return result;}
    throw Error(`Unsupported compact type ${type}`);
  }
  const result=value(12);if(offset!==buffer.length)throw Error(`Trailing footer bytes: ${buffer.length-offset}`);return result;
}
const text = value => Buffer.isBuffer(value) ? value.toString('utf8') : null;
const inventory=JSON.parse(fs.readFileSync(path.join(root,'parquet-inventory.json'),'utf8'));
const output=[];
for(const file of inventory.files) {
  const filePath=path.join(root,`${file.config}-${file.split}.footer.bin`);
  if(!fs.existsSync(filePath))continue;
  const footer=parseCompact(fs.readFileSync(filePath));
  let firstRow=0;
  const groups=footer[4].map((group,index)=>{
    const columns=group[1].map(chunk=>{
      const meta=chunk[3];
      const columnPath=meta[3].map(text).join('.');
      const stats=meta[12]??{};
      const result={
        path:columnPath, physical_type:meta[1], codec:meta[4], values:meta[5],
        compressed_bytes:meta[7], uncompressed_bytes:meta[6],
        data_page_offset:meta[9], dictionary_page_offset:meta[11]??null,
        range_start:Math.min(meta[9],meta[11]??meta[9]),
        offset_index_offset:chunk[4]??null, offset_index_length:chunk[5]??null,
        column_index_offset:chunk[6]??null, column_index_length:chunk[7]??null,
      };
      if(columnPath==='path'||columnPath==='audio.path') {
        result.minimum_filename=path.posix.basename(text(stats[6]??stats[2])??'');
        result.maximum_filename=path.posix.basename(text(stats[5]??stats[1])??'');
      }
      return result;
    });
    const result={index,first_row:firstRow,num_rows:group[3],total_uncompressed_bytes:group[2],total_compressed_bytes:group[6]??columns.reduce((n,c)=>n+c.compressed_bytes,0),columns};
    firstRow+=group[3];return result;
  });
  if(firstRow!==footer[3])throw Error('Row count mismatch');
  output.push({config:file.config,split:file.split,declared_file_bytes:file.size,footer_bytes:fs.statSync(filePath).size,num_rows:footer[3],created_by:text(footer[6]),row_groups:groups});
}
fs.writeFileSync(path.join(root,'parquet-footer-metadata.json'),JSON.stringify(output,null,2)+'\n');
console.log(JSON.stringify(output.map(f=>({config:f.config,split:f.split,rows:f.num_rows,groups:f.row_groups.length,audio_bytes:f.row_groups.flatMap(g=>g.columns).filter(c=>c.path==='audio.bytes').reduce((n,c)=>n+c.compressed_bytes,0),columns:f.row_groups[0].columns.map(c=>({path:c.path,offset_index:c.offset_index_length}))})),null,2));
