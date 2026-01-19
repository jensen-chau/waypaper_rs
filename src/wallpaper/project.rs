use std::{path::PathBuf, str::FromStr};
use std::fs::File;
use serde::{Deserialize, Serialize};
use anyhow::Result;

#[derive(Serialize, Deserialize)]
pub struct Project {
    pub description: String,

    #[serde(rename="type")]
    pub wallpaper_type: String,

    pub file: String,

    pub tags: Vec<String>,

    pub title: String,
}

pub fn build_project(path: &str) -> Result<Project> {
    let dir = PathBuf::from_str(path).unwrap();
    let project_path = dir.join("project.json");
    let project_file = File::open(project_path).unwrap();
    let project: Project = serde_json::from_reader(project_file).unwrap();
    Ok(project)
}


#[cfg(test)]
mod test {
    use super::*;
    
    #[test]
    fn test_project() {
        let path = "/home/zjx/MyDisk/SteamLibrary/steamapps/workshop/content/431960/1368637798";
 
        let project = build_project(path).unwrap();
        println!("Project title: {}", project.title);
        println!("Project type: {}", project.wallpaper_type);
        println!("Project file: {}", project.file);
    }
}
