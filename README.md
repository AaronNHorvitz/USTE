
# USTE: Universal-Spatial Temporal Engine

USTE is a GPU-accelerated graph database and analytics engine specifically designed for real-time spatial-temporal data processing and large-scale graph analytics. Combining the power of TensorFlow GPU, TimescaleDB, and ArangoDB, USTE efficiently manages complex, structured, and unstructured data. It supports advanced Graph Neural Networks (GNNs), natural language processing (NLP) via Hugging Face Transformers, and seamless integration with Large Language Models (LLMs) for enhanced data querying and analysis.

## Features

- **GPU-Accelerated Analytics:** High-speed graph and numeric computations powered by TensorFlow GPU.
- **Hybrid Data Storage:** Efficiently stores structured data in TimescaleDB and flexibly handles unstructured data in ArangoDB.
- **Graph Neural Networks (GNN):** Advanced graph modeling, including Higher-Order Graph Neural Networks, enabling sophisticated analytics and predictive modeling.
- **Embeddings & NLP Integration:** Seamless embedding generation and natural language processing with Hugging Face Transformers.
- **Spatial-Temporal Data Processing:** Optimized handling, querying, and analysis of complex spatial-temporal datasets, supporting real-time analytics and dynamic simulations.
- **LLM Integration:** Direct integration of Large Language Models to enhance query capabilities, automate analysis of unstructured data, and facilitate advanced scenario modeling.
- **GPU-Accelerated Bayesian Modeling:** High-performance Bayesian inference, including Markov Chain Monte Carlo (MCMC), leveraging TensorFlow Probability for rapid probabilistic modeling.
- **Real-time APIs:** Fast, scalable API endpoints via FastAPI for immediate data access and real-time interaction.
- **Containerized Deployment:** Dockerized architecture ensures portability, scalability, and ease of deployment across various environments.

## Project Structure

```
USTE/
├── docker-compose.yml        # Docker Compose configuration for TimescaleDB and ArangoDB
├── README.md                 # Project documentation
├── requirements.txt          # Python dependencies
├── data/                     # Persistent database storage
│   ├── arango/
│   └── timescale/
├── images/                   # Project-related images for documentation
├── notebooks/                # Jupyter notebooks for exploration and prototyping
├── sandbox/                  # Experimental scripts and temporary tests
├── src/                      # Main application source code
│   ├── app/
│   │   ├── main.py           # FastAPI application entry point
│   │   ├── models.py         # Data models and schemas
│   │   └── utils.py          # Utility functions
│   ├── database/
│   │   └── db.py             # Database connection and helper functions
│   └── embeddings/
│       └── embedding.py      # Embedding generation code (Hugging Face, TensorFlow)
└── tests/                    # Unit and integration tests
    └── test_app.py
```

## Installation

### Step 1: Clone the Repository

```bash
git clone https://github.com/yourusername/uste.git
cd uste
```

### Step 2: Dockerized Database Setup

Ensure Docker is installed, then launch containers:

```bash
docker compose up -d
```

- TimescaleDB: Accessible at `localhost:5432` (user: `postgres`, password: `password`)
- ArangoDB: Accessible at `http://localhost:8529` (user: `root`, password: `password`)

### Step 3: Set Up Python Environment

Create and activate your environment:

```bash
conda create -n uste_env python=3.12 -y
conda activate uste_env
pip install -r requirements.txt
```

## Usage

### Start FastAPI Application

```bash
uvicorn src.app.main:app --reload
```

### Example API Endpoints

- `GET /status`: Health check and database connectivity
- `POST /nodes`: Creates a new node in the graph database.
- `GET /nodes/{node_id}`: Retrieves a specific node by its ID.
- `POST /edges`: Creates an edge to define a relationship between two nodes.
- `POST /metrics`: Ingests a new time-series data point.
- `GET /`: Root endpoint for basic API health check.

## Testing

Run the full test suite:

```bash
pytest tests/
```

## Contributing

Your contributions are warmly welcomed! Please:

- Fork the repository.
- Create a branch (`git checkout -b feature/new-feature`).
- Submit a pull request for review.

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.

## Contact

- Email: [your.email@example.com](mailto:your.email@example.com)
- GitHub Issues: Please open a new issue on GitHub.

## Adding Images to README

Store images in the `images/` directory, and reference them in Markdown:

```markdown
![Image Description](images/your-image.png)
```