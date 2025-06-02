# USTE: Universal-Spatial Temporal Engine - Architecture

This document outlines the architecture of the Universal-Spatial Temporal Engine (USTE), detailing its file and folder structure, the purpose of each component, how state is managed, and how its services connect.

## 1. Overview

USTE is a GPU-accelerated graph database and analytics engine designed for real-time spatial-temporal data processing and large-scale graph analytics. It integrates TensorFlow GPU, TimescaleDB, and ArangoDB to manage structured and unstructured data. Key capabilities include Graph Neural Networks (GNNs), NLP via Hugging Face Transformers, LLM integration, GPU-accelerated Bayesian modeling, and real-time APIs via FastAPI. The system is designed for containerized deployment using Docker.

## 2. File and Folder Structure

```
USTE/
├── docker-compose.yml      # Docker Compose configuration for TimescaleDB and ArangoDB
├── README.md               # Project documentation
├── requirements.txt        # Python dependencies
├── data/                   # Persistent database storage
│   ├── arango/             # Persistent storage for ArangoDB
│   └── timescale/          # Persistent storage for TimescaleDB
├── images/                 # Project-related images for documentation
├── notebooks/              # Jupyter notebooks for exploration and prototyping
├── sandbox/                # Experimental scripts and temporary tests
├── src/                    # Main application source code
│   ├── app/                # Core application and API layer
│   │   ├── main.py         # FastAPI application entry point, defines API routes
│   │   ├── models.py       # Data models, Pydantic schemas for API validation
│   │   └── utils.py        # Utility functions for the application
│   ├── database/           # Database interaction layer
│   │   └── db.py           # Database connection logic and helper functions for TimescaleDB & ArangoDB
│   └── embeddings/         # Embedding generation and management
│       └── embedding.py    # Code for generating embeddings (Hugging Face, TensorFlow)
└── tests/                  # Unit and integration tests
└── test_app.py         # Tests for the FastAPI application and its endpoints
```

## 3. Component Descriptions

### Root Directory (`USTE/`)

* **`docker-compose.yml`**:
    * **Purpose**: Defines and configures the multi-container Docker application, specifically for spinning up and managing the TimescaleDB and ArangoDB services.
    * **Functionality**: Ensures that the databases are run in isolated environments with persistent storage and defined network configurations, making setup and deployment consistent.

* **`README.md`**:
    * **Purpose**: Provides a comprehensive overview of the project, including its features, installation instructions, usage guidelines, and contribution information.
    * **Functionality**: Serves as the primary entry point for developers and users to understand and get started with USTE.

* **`requirements.txt`**:
    * **Purpose**: Lists all Python packages and their versions required for the project to run.
    * **Functionality**: Used by `pip` to install dependencies, ensuring a consistent Python environment.

### `data/` Directory

* **Purpose**: Provides mount points for persistent storage for the databases, ensuring data is not lost when Docker containers are stopped or restarted.
* **`data/arango/`**: Stores data files for the ArangoDB instance.
* **`data/timescale/`**: Stores data files for the TimescaleDB instance.

### `images/` Directory

* **Purpose**: Contains static image files (e.g., diagrams, screenshots) used in the `README.md` or other documentation.

### `notebooks/` Directory

* **Purpose**: Houses Jupyter notebooks.
    * **Functionality**: Used for exploratory data analysis, prototyping machine learning models (GNNs, NLP), developing algorithms, and visualizing data before integrating them into the main application.

### `sandbox/` Directory

* **Purpose**: A directory for experimental scripts, temporary tests, or proof-of-concept code.
    * **Functionality**: Allows developers to try out new ideas or libraries without cluttering the main source code or test suites.

### `src/` Directory (Main Application Source Code)

* **Purpose**: Contains all the core logic for the USTE application.

    * **`src/app/`**: Handles the API layer and primary application logic.
        * **`main.py`**: The entry point for the FastAPI web application. It defines API endpoints (e.g., `/status`, `/nodes`, `/graph`), handles incoming requests, and orchestrates responses.
        * **`models.py`**: Defines Pydantic models for request and response data validation and serialization. It may also include other data structures or schemas used within the application.
        * **`utils.py`**: Contains general-purpose utility functions used across the `app` module to promote code reusability and maintainability.

    * **`src/database/`**: Manages all interactions with the databases.
        * **`db.py`**: Contains functions to establish connections to TimescaleDB and ArangoDB. It abstracts database query logic (e.g., CRUD operations, complex graph queries, spatial-temporal queries) and provides a clean interface for the rest of the application to interact with the data stores.

    * **`src/embeddings/`**: Responsible for generating and managing data embeddings.
        * **`embedding.py`**: Implements the logic for creating vector embeddings from data (e.g., text, graph nodes). It leverages libraries like Hugging Face Transformers for NLP tasks and potentially TensorFlow for other embedding techniques. These embeddings are crucial for GNNs and semantic search/querying.

### `tests/` Directory

* **Purpose**: Contains all automated tests for the application.
    * **`test_app.py`**: Includes unit and integration tests for the FastAPI application, ensuring API endpoints function as expected, data validation works correctly, and core logic is sound. Tests are typically run using `pytest`.

## 4. State Management

State in USTE is managed across several components:

* **Persistent State (Primary Data Stores):**
    * **TimescaleDB**:
        * **Role**: Stores structured data, particularly time-series and spatial-temporal data.
        * **Location**: Data is persisted on disk within the Docker volume mapped to `data/timescale/`.
    * **ArangoDB**:
        * **Role**: Stores unstructured data, graph data (nodes, edges, their properties), and potentially document-based information.
        * **Location**: Data is persisted on disk within the Docker volume mapped to `data/arango/`.

* **In-Memory State (Processing & Computation):**
    * **FastAPI Application (`src/app/main.py`)**: Primarily stateless, but can hold temporary state related to ongoing requests or maintain short-lived caches for performance.
    * **TensorFlow & Hugging Face Transformers (`src/embeddings/embedding.py`, and implied analytics modules):**
        * **Role**: These components process data in memory (CPU RAM and GPU VRAM for TensorFlow). This includes loading data from databases, building and training GNNs, performing NLP tasks, generating embeddings, and running Bayesian models.
        * **Location**: Intermediate results and models are held in memory during computation. Trained models might be serialized and persisted back to disk (potentially in a dedicated models directory or a database).
    * **LLMs (Large Language Models):**
        * **Role**: If local LLMs are used, they will consume significant memory. If cloud-based LLMs are used, state related to the API interaction (e.g., context windows) is managed by the LLM service, with USTE handling the local context.

* **Ephemeral State:**
    * **Docker Containers**: The running instances of TimescaleDB, ArangoDB, and potentially the FastAPI application (if containerized for production) have ephemeral state related to their runtime processes.

## 5. Service Connections & Data Flow

1.  **Client Interaction**:
    * External users or services interact with USTE via **Real-time APIs** exposed by the **FastAPI application** (`src/app/main.py`). Requests are typically HTTP-based (e.g., GET, POST).

2.  **API Layer Processing (FastAPI)**:
    * `src/app/main.py` receives requests and uses Pydantic models from `src/app/models.py` for data validation.
    * Based on the endpoint, it orchestrates calls to other internal services or modules.

3.  **Database Interaction (`src/database/db.py`)**:
    * The FastAPI application, through `src/database/db.py`, connects to:
        * **TimescaleDB** (accessible at `localhost:5432` as per README) for operations on structured spatial-temporal data (e.g., storing sensor readings, trajectories).
        * **ArangoDB** (accessible at `http://localhost:8529` as per README) for graph operations (creating nodes/edges, querying graph structures using AQL), and managing unstructured or semi-structured documents.

4.  **Analytics and Computation Engine**:
    * **Embedding Generation (`src/embeddings/embedding.py`)**: For tasks requiring embeddings (e.g., NLP, GNN input), this module uses **Hugging Face Transformers** or **TensorFlow** to convert data (fetched from databases) into vector representations.
    * **GPU-Accelerated Analytics (TensorFlow)**: For GNNs, Bayesian modeling (MCMC via TensorFlow Probability), and other numeric computations, USTE leverages **TensorFlow GPU**. Data is typically loaded from TimescaleDB/ArangoDB, processed on the GPU, and results might be stored back or returned via the API.
    * **Spatial-Temporal Processing**: Logic (likely within `src/` but not explicitly detailed as a separate file) handles specialized queries and analysis of spatial-temporal data, combining capabilities of TimescaleDB (for efficient time-series and spatial queries) and graph analytics (for relationships).

5.  **LLM Integration**:
    * USTE integrates with **Large Language Models**. This can involve:
        * Sending data (e.g., unstructured text from ArangoDB, user queries) to an LLM API (either external or a locally hosted model).
        * Receiving processed information, insights, or generated queries from the LLM.
        * Using LLM outputs to enhance data querying (natural language to database queries), automate analysis, or facilitate scenario modeling.

6.  **Containerization (Docker)**:
    * **`docker-compose.yml`** manages the **TimescaleDB** and **ArangoDB** services. These services communicate over a Docker-managed network.
    * The FastAPI application (run via `uvicorn` on the host during development) connects to these database services via their exposed ports (`localhost:5432` for TimescaleDB, `localhost:8529` for ArangoDB). In a fully containerized production setup, the FastAPI app would also be a Docker service on the same network.

### Simplified Data Flow Example (Adding a Node):

1.  Client sends `POST /nodes` request with node data to FastAPI.
2.  FastAPI (`main.py`) validates data using `models.py`.
3.  If node involves text for NLP, FastAPI may call `embedding.py` to generate embeddings using Hugging Face.
4.  FastAPI calls `db.py` to store the node and its properties (including embeddings) in ArangoDB.
5.  `db.py` executes the AQL query against ArangoDB.
6.  FastAPI returns a success/failure response to the client.

### Service Connection Diagram

+---------------+
|  FastAPI      |
|  Application  |
+---------------+
       |
       |
       v
+---------------+
|  Database     |
|  (TimescaleDB |
|  and ArangoDB)|
+---------------+
       |
       |
       v
+---------------+
|  Embedding    |
|  Generation   |
|  (Hugging Face|
| Transformers) |
+---------------+
       |
       |
       v
+---------------+
|  GPU          |
|  Acceleration |
|  (TensorFlow  |
|   GPU)        |
+---------------+
