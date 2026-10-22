FROM python:3.12-slim-bookworm
WORKDIR /app
COPY services/trainer/requirements.lock /app/requirements.lock
RUN pip install --no-cache-dir -r requirements.lock
COPY services/trainer/app.py /app/app.py
RUN useradd --system --uid 10001 trainer
USER trainer
ENV OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1
CMD ["uvicorn","app:app","--host","0.0.0.0","--port","8001","--limit-concurrency","8"]
