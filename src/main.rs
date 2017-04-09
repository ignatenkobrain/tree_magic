#[macro_use] extern crate nom;

extern crate petgraph;
extern crate clap;

use std::io::prelude::*;
use std::io::BufReader;
use std::fs::File;
use std::collections::HashMap;
use std::fs;
use petgraph::prelude::*;
use clap::{Arg, App};

mod parse;
use parse::magic;


// Get list of known system filetypes
fn mimelist_init() -> Result<Vec<String>, std::io::Error> {
    let ftypes = File::open("/usr/share/mime/types")?;
    let rtypes = BufReader::new(ftypes);
    let mut mimelist = Vec::<String>::new();
    
    for line in rtypes.lines() {
        let mime = line?.split_whitespace().nth(0).unwrap_or("").to_string();
        mimelist.push(mime);
    }
    
    let mimelist = mimelist;
    Ok(mimelist)
}

// Get filetype aliases
fn aliaslist_init() -> Result<HashMap<String, String>, std::io::Error> {
    let faliases = File::open("/usr/share/mime/aliases")?;
    let raliases = BufReader::new(faliases);
    let mut aliaslist = HashMap::<String, String>::new();
    
    for line in raliases.lines() {
        let line_raw = line?;
    
        let a = line_raw.split_whitespace().nth(0).unwrap_or("").to_string();
        let b = line_raw.split_whitespace().nth(0).unwrap_or("").to_string();
        aliaslist.insert(a,b);
    }
    
    let aliaslist = aliaslist;
    Ok(aliaslist)
}

// Initialize filetype graph
fn graph_init() -> Result<DiGraph<String, u32>, std::io::Error> {

    let fsubclasses = File::open("/usr/share/mime/subclasses")?;
    let rsubclasses = BufReader::new(fsubclasses);
    
    let mut graph = DiGraph::<String, u32>::new();
    let mut added_mimes = HashMap::<String, NodeIndex>::new();
    
    let mut node_text: NodeIndex = NodeIndex::default();
    let mut node_octet: NodeIndex = NodeIndex::default();
    let mut node_allall: NodeIndex = NodeIndex::default();
    let mut node_allfiles: NodeIndex = NodeIndex::default();
    
    // Get list of MIME types
    let mimelist = mimelist_init()?;
    // Get list of MIME aliases (doesn't need to exist.)
    let aliaslist = aliaslist_init().unwrap_or(HashMap::<String, String>::new());
    
    // Create all nodes
    for mimetype in mimelist.iter() {
    
        // Do not insert aliases
        let mut mimetype = mimetype;
        match aliaslist.get(mimetype) {
            Some(alias) => {mimetype = alias;}
            None => {}
        }
        let mimetype = mimetype;
        
        // Do not insert "x-content/*" or "multipart/*"
        let toplevel = mimetype.split("/").nth(0).unwrap_or("");
        if toplevel == "x-content" || toplevel == "multipart" {
            continue;
        }
    
        let node = graph.add_node(mimetype.clone());
        added_mimes.insert(mimetype.clone(), node);
        
        // Record well-used parent types now
        if mimetype == "text/plain" {
            node_text = node;
        } else if mimetype == "application/octet-stream" {
            node_octet = node;
        } else if mimetype == "all/all" {
            node_allall = node;
        } else if mimetype == "all/allfiles" {
            node_allfiles = node;
        }
    }
    
    if node_text == NodeIndex::default() {
        let mimetype = "text/plain".to_string();
        node_text = graph.add_node(mimetype.clone());
        added_mimes.insert(mimetype.clone(), node_text);
    }
    
    if node_octet == NodeIndex::default() {
        let mimetype = "application/octet-stream".to_string();
        node_octet = graph.add_node(mimetype.clone());
        added_mimes.insert(mimetype.clone(), node_octet);
    }
    
    let node_text = node_text;
    let node_octet = node_octet;

    
    // If a relation exists, add child to parent node
    for line in rsubclasses.lines() {
        let line_raw = line?;
        let mut child_raw = line_raw.split_whitespace().nth(0).unwrap_or("").to_string();
        let mut parent_raw = line_raw.split_whitespace().nth(1).unwrap_or("").to_string();
        
        // If child or parent refers to an alias, change it to the real type
        match aliaslist.get(&child_raw) {
            Some(alias) => {child_raw = alias.clone();}
            None => {}
        }
        match aliaslist.get(&parent_raw) {
            Some(alias) => {parent_raw = alias.clone();}
            None => {}
        }
        let child_raw = child_raw;
        let parent_raw = parent_raw;
        
        let parent: NodeIndex;
        let child: NodeIndex;
        
        match added_mimes.get(&parent_raw) {
            Some(node) => {parent = *node;}
            None => {continue;}
        }
        
        match added_mimes.get(&child_raw) {
            Some(node) => {child = *node;}
            None => {continue;}
        }
        
        graph.update_edge(parent, child, 1);
    }
    
    
    //Otherwise, add to applicaton/octet-stream, all/all, or text/plain, depending on top-level
    graph.update_edge(node_octet, node_text, 1);
    graph.update_edge(node_allall, node_allfiles, 1);
    graph.update_edge(node_allfiles, node_octet, 1);
    
    let mut edge_list = Vec::<(NodeIndex, NodeIndex)>::new();
    for mimenode in graph.externals(Incoming) {
        
        let ref mimetype = graph[mimenode];
        let toplevel = mimetype.split("/").nth(0).unwrap_or("");
        
        if mimenode == node_text || mimenode == node_octet || 
           mimenode == node_allfiles || mimenode == node_allall 
        {
            continue;
        }
        
        if toplevel == "text" {
            edge_list.push( (node_text, mimenode) );
        } else if toplevel == "inode" {
            edge_list.push( (node_allall, mimenode) );
        } else {
            edge_list.push( (node_octet, mimenode) );
        }
    }
    
    graph.extend_with_edges(edge_list);
    
    let graph = graph;
    //println!("{:?}", Dot::with_config(&graph, &[Config::EdgeNoLabel]));

    Ok(graph)
}

/// The meat. Gets the type of a file.
fn get_type_from_filepath(
    node: Option<NodeIndex>,
    typegraph: DiGraph<String, u32>, 
    magic_ruleset: Vec<magic::MagicEntry>,
    filepath: &str
) -> Option<String> {

    // Start at an outside unconnected node if no node given
    let parentnode: NodeIndex;
    
    //println!{">>"};
    
    match node {
        Some(foundnode) => parentnode = foundnode,
        None => {
            match typegraph.externals(Incoming).next() {
                Some(foundnode) => parentnode = foundnode,
                None => panic!("No external nodes found!")
            }
        }
    }
    
    // Walk the children
    let mut children = typegraph.neighbors_directed(parentnode, Outgoing).detach();
    while let Some(childnode) = children.next_node(&typegraph) {
        let mimetype = typegraph[childnode].clone();
        let rule: magic::MagicEntry;
        
        //println!("{}", mimetype);
        
        // Test the 
        /*if  (mimetype == "all/allfiles" ||
            mimetype == "application/octet-stream")
        {
            return get_type_from_filepath(
                Some(childnode), typegraph, magic_ruleset, filepath
            );
        }*/
        
        
        let meta = fs::metadata(filepath);
        match meta {
            Ok(meta) => {
                if mimetype == "inode/directory" && meta.is_dir() {
                    return Some(mimetype);
                } else if mimetype == "all/allfiles" && meta.is_file() {
                    return get_type_from_filepath(
                        Some(childnode), typegraph, magic_ruleset, filepath
                    );
                } else if !meta.is_file() {
                    // TODO: handle different inodes
                    return None;
                }
            },
            Err(_) => {continue;}
        }
        
        // Automatically forward application/octet-stream
        if mimetype == "application/octet-stream" {
            return get_type_from_filepath(
                Some(childnode), typegraph, magic_ruleset, filepath
            );
        }
        
        // TODO: Handle text/plain. because that's totally broken right now
        
        match magic_ruleset.binary_search_by(|x| x.mime.cmp(&mimetype)) {
            Ok(idx) => rule = magic_ruleset[idx].clone(),
            Err(_) => {continue;},
        }
        
        match magic::test::from_filepath(filepath, rule) {
            Ok(res) => match res {
                true => {
                    match get_type_from_filepath(
                        Some(childnode), typegraph, magic_ruleset, filepath
                    ) {
                        Some(foundtype) => return Some(foundtype),
                        None => return Some(mimetype.clone()),
                    }
                }
                false => continue,
            },
            Err(why) => panic!("{:?}", why),
        }
    }
    
    None
}

fn main() {

    let args = App::new("TreeMagic")
        .version("0.1")
        .about("Finds the MIME type of a file using FD.O Shared MIME database")
        .arg(Arg::with_name("file")
            .required(true)
            .index(1)
            .multiple(true)
        )
        .get_matches();
    let files: Vec<_> = args.values_of("file").unwrap().collect();

    let typegraph: DiGraph<String, u32>;
    match graph_init() {
        Err(why) => panic!("{:?}", why),
        Ok(out) => {
            typegraph = out;
        },
    };
    
    let magic_ruleset: Vec<magic::MagicEntry>;
    match magic::ruleset::from_filepath("/usr/share/mime/magic") {
        Err(why) => panic!("{:?}", why),
        Ok(out) => {
            magic_ruleset = out;
        },
    }
    
    for x in files {
       println!("{}:\t{:?}", x, get_type_from_filepath(None, typegraph.clone(), magic_ruleset.clone(), x).unwrap_or("inode/none".to_string()));
    }
    
}
