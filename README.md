<div align="center">
<h1>chunkr</h1>
<h3>A fast and quick chunking library for 🦀</h3>

[![Latest version](https://img.shields.io/crates/v/chunkr.svg)](https://crates.io/crates/chunkr)
![License](https://img.shields.io/crates/l/chunkr.svg)

</div>

The project aims to help Rust developers build text and language-based applications that utilize some kind of documents or text. It is built for developers to chunkify large documents into smaller chunks without using heavy resources.

use `chunkr` to split large *pdf* documents into smaller chunks for LLM training and RAG (Retrieval Augmented Generation) application development. 

## 🚀 Getting Started

To add [`chunkr`](https://crates.io/crates/chunkr) to your project and start chunking, use the **cargo** cli
```bash
cargo add chunkr
```

There are some examples mentioned in the `examples` directory. Checkout those to get started. 


### To checkout code and build it yourself

Clone the repository and run one of the *examples* from the `examples` directory. 

```bash
git clone https://github.com/d1pankarmedhi/chunkr.git
cd chunkr
```

## 🏗️ Examples

Check out these examples to quickly get started:

### Chunking

These are some chunking strategy examples:

- [Chunking by words](/examples/chunk_by_words.rs) - Chunk your documents/texts by number of words. 
- [Chunking by characters](/examples/chunk_by_chars.rs) - Chunk your documents/text by number of characters.
- [Chunk PDF document](/examples/chunk_document.rs) - Chunk your pdf documents by words/characters.

Run them using the cargo command like:
```bash
# cargo run --example example-name chunk-size overlap file-path
cargo run --example chunk_document 1000 20 /home/home/Downloads/clean_code.pdf
```

## 💡 Contributing
As an open-source project, we are open to all kinds of contributions, be it through code, documentation, issues, bugs, or even feature suggestions. 

Feel free to check out [Contribution](/CONTRIBUTION.md) guide for more details.

## 📝 License

This project is licensed under the MIT License - see the [LICENSE.md](LICENSE.md) file for details

<!-- ### Loader

These are some document loader examples: -->


<!-- 
### Running the tests

Run the tests and check if there are any error or failed tests. 


### And coding style tests

Explain what these tests test and why

```
Give an example
```

## Deployment

Add additional notes about how to deploy this on a live system

## Built With

* [Dropwizard](http://www.dropwizard.io/1.0.2/docs/) - The web framework used
* [Maven](https://maven.apache.org/) - Dependency Management
* [ROME](https://rometools.github.io/rome/) - Used to generate RSS Feeds

## Contributing

Please read [CONTRIBUTING.md](https://gist.github.com/PurpleBooth/b24679402957c63ec426) for details on our code of conduct, and the process for submitting pull requests to us.

## Versioning

We use [SemVer](http://semver.org/) for versioning. For the versions available, see the [tags on this repository](https://github.com/your/project/tags). 

## Authors

* **Billie Thompson** - *Initial work* - [PurpleBooth](https://github.com/PurpleBooth)

See also the list of [contributors](https://github.com/your/project/contributors) who participated in this project.

## License

This project is licensed under the MIT License - see the [LICENSE.md](LICENSE.md) file for details

## Acknowledgments

* Hat tip to anyone whose code was used
* Inspiration
* etc -->