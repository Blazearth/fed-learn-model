FROM python:3.12-slim

WORKDIR /app

# Non-root user for security
RUN adduser --disabled-password --gecos "" appuser

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY aggregator.py .

USER appuser

CMD ["python", "aggregator.py"]
