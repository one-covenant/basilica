FROM pytorch/pytorch:2.13.0-cuda12.6-cudnn9-runtime@sha256:6acf597eeb8e376a96580dde4952f37cc017fef732bb40bfc73f28f25e3f64b4

# Create non-root user (required by Basilica)
RUN useradd -m -u 1000 appuser

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    && rm -rf /var/lib/apt/lists/*

# Install the project's declared dependencies against the patched CUDA image.
COPY pyproject.toml README.md ./
COPY --chown=appuser:appuser src/ ./src/
RUN pip install --no-cache-dir . \
    && python -c 'import torch; assert torch.__version__.split("+", 1)[0] == "2.13.0", torch.__version__; assert torch.version.cuda == "12.6", torch.version.cuda'

# Copy the launcher after dependency installation for build caching.
COPY --chown=appuser:appuser train.py .
COPY --chown=appuser:appuser run.sh .
COPY --chown=appuser:appuser entrypoint.py .

# Set PYTHONPATH so the package is importable
ENV PYTHONPATH=/app/src

USER appuser

EXPOSE 8000

CMD ["python", "entrypoint.py"]
