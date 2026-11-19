"""Local model comparison of manual Wobbegong lookup and pre-submit enrichment.
Usage: python3 qa/benchmark_enrichment.py BINARY MODEL_DIRECTORY
Uses synthetic documents, greedy CPU decoding, and a temporary Rhyven home.
"""
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
from time import perf_counter
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

binary, model_dir = str(Path(sys.argv[1]).resolve()), sys.argv[2]
torch.set_num_threads(2)
tokenizer = AutoTokenizer.from_pretrained(model_dir, local_files_only=True)
model = AutoModelForCausalLM.from_pretrained(model_dir, local_files_only=True).eval()


def generate(messages, limit):
    ids = tokenizer.apply_chat_template(messages, add_generation_prompt=True, return_tensors="pt")
    with torch.inference_mode():
        output = model.generate(ids, attention_mask=torch.ones_like(ids), do_sample=False,
                                max_new_tokens=limit, pad_token_id=tokenizer.eos_token_id)
    response = output[0,ids.shape[1]:]
    return tokenizer.decode(response, skip_special_tokens=True).strip(), ids.shape[1] + len(response)


def benchmark(home):
    env = dict(os.environ,RHYVEN_HOME=home)
    def cli(*args,payload=None):
        raw=subprocess.run([binary,*args],env=env,input=json.dumps(payload),text=True,
                           check=True,capture_output=True)
        return json.loads(raw.stdout)
    def action(name,args):
        return cli("call","rhyven_call","-",payload={"category":"rhyven/wobbegong",
                                                    "function":"action_"+name,"args":args})
    fixtures=[("amber handoff","KESTREL"),("violet release","HERON"),("silver backup","PLOVER")]
    action("ingest_files",{"source":"test","documents":[{"id":topic,"text":f"The {topic} code is {word}."}
                                                        for topic,word in fixtures]})
    cli("context","enable","bench")
    results=[]
    for topic,expected in fixtures:
        prompt=f"What is the {topic} code?"
        for mode in ("manual","automatic"):
            start=perf_counter()
            tokens=0
            query=None
            if mode=="manual":
                query, tokens=generate([{"role":"system","content":"Return only two search keywords from the question, without punctuation."},
                                        {"role":"user","content":"What is the copper rollout code?"},
                                        {"role":"assistant","content":"copper rollout"},
                                        {"role":"user","content":prompt}],8)
                result=action("query",{"search":query[:256] or topic,"limit":3})
                data=json.dumps(result["items"],separators=(",",":"))
            else:
                packet=action("prepare_context",{"prompt":prompt,"turn":topic,"connection":"bench","tool":"rhyven_call"})
                data=packet["addition"] or ""
            answer,cost=generate([{"role":"system","content":"Answer with the single code word from the evidence. If missing, answer UNKNOWN."},
                                  {"role":"user","content":prompt+"\nEvidence:\n"+data}],12)
            results.append(dict(mode=mode,topic=topic,query=query,answer=answer,correct=expected in re.findall(r"[A-Z]+",answer.upper()),
                                total_tokens=tokens+cost,seconds=round(perf_counter()-start,3)))
    return results


with tempfile.TemporaryDirectory() as home:
    print(json.dumps(benchmark(home),indent=2))
