#!/bin/bash
set -e

# env variables
AWS_ACCOUNT_ID=053744071699 #this isn't confidential according to AWS https://www.lastweekinaws.com/blog/are-aws-account-ids-sensitive-information/
AWS_REGION=us-west-2
REPOSITORY_NAME=txcrimemapper
IMAGE_TAG=latest

# build docker image
docker build -t $REPOSITORY_NAME .

# tag docker image for ECR
docker tag $REPOSITORY_NAME:latest $AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com/$REPOSITORY_NAME:$IMAGE_TAG

# authenticate to ECR registry
aws ecr get-login-password --region $AWS_REGION | docker login --username AWS --password-stdin $AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com

# push docker image to ECR
docker push $AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com/$REPOSITORY_NAME:$IMAGE_TAG
